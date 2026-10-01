//! The real sky, offscreen: stars, planets, eclipses, constellation figures.
//! `cargo run -p nus-render --release --example celestial -- <dir> [case ...]`
//! Cases are named below; with none given, all are written as PNGs.
use nus_astro::{sky_frame, Observer, SkyFrame};
use nus_render::sky::{Celestial, Mark};
use nus_render::{SkyParams, SkyRenderer};
use std::path::Path;

fn utc(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> f64 {
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if mo > 2 { mo - 3 } else { mo + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    ((days * 86_400 + h * 3600 + mi * 60 + s) * 1000) as f64
}

struct Shot {
    name: &'static str,
    ms: f64,
    place: (f64, f64),
    /// Look at: "sun", "moon", or a bearing/elevation in degrees.
    look: Look,
    fov: f32,
    lines: f32,
    cover: f32,
}

enum Look {
    Sun,
    Moon,
    Sky(f32, f32),
}

fn params(frame: &SkyFrame, shot: &Shot) -> SkyParams {
    let mut p = SkyParams {
        sun_direction: frame.sun_direction,
        moon_direction: frame.moon_direction,
        moon_illumination: frame.moon_illumination,
        moon_waxing: frame.moon_waxing,
        low_cover: shot.cover,
        high_cover: 0.0,
        mid_cover: 0.0,
        fov_y: shot.fov.to_radians(),
        ..Default::default()
    };
    let aim = |d: [f32; 3]| (d[0].atan2(d[2]), d[1].asin());
    let (bearing, elevation) = match shot.look {
        Look::Sun => aim(frame.sun_direction),
        Look::Moon => aim(frame.moon_direction),
        Look::Sky(b, e) => (b.to_radians(), e.to_radians()),
    };
    p.view_azimuth = bearing;
    p.view_elevation = elevation.max(0.15);
    let mut planets = [None; nus_render::sky::MAX_PLANETS];
    for (slot, m) in planets.iter_mut().zip(&frame.planets) {
        *slot = Some(Mark {
            direction: m.direction,
            magnitude: m.magnitude,
            bv: m.bv,
        });
    }
    p.celestial = Some(Celestial {
        rotation: frame.rotation,
        planets,
        sun_radius: frame.sun_radius,
        moon_radius: frame.moon_radius,
        sun_visible: frame.sun_visible,
        corona: frame.corona,
        ecliptic_north: frame.ecliptic_north,
        shadow_direction: frame.shadow_direction,
        umbra_radius: frame.umbra_radius,
        penumbra_radius: frame.penumbra_radius,
        moon_glow: frame.moon_glow,
        lines: shot.lines,
        clear_sky: 0.0,
    });
    p
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let directory = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "/tmp/nus-celestial".into());
    std::fs::create_dir_all(&directory)?;
    let burgos = (42.34, -3.70);
    // Totality at Burgos is 2026-08-12 18:28:13 – 18:30:00 UT.
    let totality = utc(2026, 8, 12, 18, 29, 7);
    let shots = [
        Shot {
            name: "night-new-york",
            ms: utc(2026, 10, 1, 4, 0, 0),
            place: (40.7, -74.0),
            look: Look::Sky(180.0, 40.0),
            fov: 70.0,
            lines: 0.0,
            cover: 0.05,
        },
        Shot {
            name: "night-constellations",
            ms: utc(2026, 10, 1, 4, 0, 0),
            place: (40.7, -74.0),
            look: Look::Sky(180.0, 40.0),
            fov: 70.0,
            lines: 1.0,
            cover: 0.05,
        },
        Shot {
            name: "night-orion-winter",
            ms: utc(2026, 12, 20, 4, 0, 0),
            place: (40.7, -74.0),
            look: Look::Sky(180.0, 40.0),
            fov: 60.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "night-southern",
            ms: utc(2026, 6, 20, 14, 0, 0),
            place: (-33.9, 151.2),
            look: Look::Sky(190.0, 55.0),
            fov: 80.0,
            lines: 1.0,
            cover: 0.0,
        },
        Shot {
            name: "twilight-venus",
            ms: utc(2026, 10, 1, 23, 35, 0),
            place: (51.5, -0.1),
            look: Look::Sky(230.0, 18.0),
            fov: 56.0,
            lines: 0.0,
            cover: 0.1,
        },
        Shot {
            name: "eclipse-before",
            ms: utc(2026, 8, 12, 17, 20, 0),
            place: burgos,
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "eclipse-partial-50",
            ms: utc(2026, 8, 12, 17, 58, 0),
            place: burgos,
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "eclipse-partial-90",
            ms: utc(2026, 8, 12, 18, 18, 0),
            place: burgos,
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "eclipse-partial-99",
            ms: utc(2026, 8, 12, 18, 26, 30),
            place: burgos,
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "eclipse-diamond",
            ms: utc(2026, 8, 12, 18, 28, 8),
            place: burgos,
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "eclipse-totality",
            ms: totality,
            place: burgos,
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "eclipse-totality-wide",
            ms: totality,
            place: burgos,
            look: Look::Sky(280.0, 25.0),
            fov: 70.0,
            lines: 0.0,
            cover: 0.15,
        },
        Shot {
            name: "eclipse-dallas-2024",
            ms: utc(2024, 4, 8, 18, 42, 40),
            place: (32.78, -96.80),
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "annular-2023",
            ms: utc(2023, 10, 14, 17, 55, 0),
            place: (29.4, -100.0),
            look: Look::Sun,
            fov: 9.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "lunar-total",
            ms: utc(2025, 3, 14, 7, 0, 0),
            place: (40.0, -100.0),
            look: Look::Moon,
            fov: 10.0,
            lines: 0.0,
            cover: 0.0,
        },
        Shot {
            name: "lunar-partial",
            ms: utc(2025, 3, 14, 5, 40, 0),
            place: (40.0, -100.0),
            look: Look::Moon,
            fov: 10.0,
            lines: 0.0,
            cover: 0.0,
        },
    ];
    let only: Vec<&String> = args.iter().skip(2).collect();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else {
            wgpu::Backends::VULKAN | wgpu::Backends::DX12
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    println!("adapter={:?}", adapter.get_info());
    let size = (1280, 820);
    for shot in &shots {
        if !only.is_empty() && !only.iter().any(|n| n.as_str() == shot.name) {
            continue;
        }
        let mut sky = SkyRenderer::new_offscreen(&device, &queue);
        let frame = sky_frame(shot.ms, &Observer::new(shot.place.0, shot.place.1));
        let p = params(&frame, shot);
        sky.render_offscreen(size, p);
        let image = capture(&device, &queue, sky.output_texture().unwrap())?;
        assert!(
            image.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
            "output must be opaque"
        );
        let (w, h) = sky.stats().output_size;
        std::fs::write(
            Path::new(&directory).join(format!("{}.png", shot.name)),
            nus_render::icon::png(&image, w, h),
        )?;
        println!(
            "{:<22} sun_visible={:.4} corona={:.2} umbra={:.4} planets_up={}",
            shot.name,
            frame.sun_visible,
            frame.corona,
            frame.umbra_radius,
            frame.planets.iter().filter(|m| m.altitude > 0.0).count()
        );
    }
    Ok(())
}

fn capture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> anyhow::Result<Vec<u8>> {
    let size = texture.size();
    let stride = (size.width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("celestial capture"),
        size: u64::from(stride) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;
    let mapped = slice.get_mapped_range()?;
    let mut bytes = Vec::with_capacity((size.width * size.height * 4) as usize);
    for row in mapped.chunks_exact(stride as usize) {
        bytes.extend_from_slice(&row[..size.width as usize * 4]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(bytes)
}
