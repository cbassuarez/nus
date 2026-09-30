//! Native offscreen sky captures and bounded timestamp diagnostics.
//! `cargo run -p nus-render --release --example sky -- /tmp/nus-sky`
//! Uses a separate device; enabling timestamps here does not change app features.
use nus_render::{SkyParams, SkyRenderer};
use std::{path::Path, time::Instant};
fn main() -> anyhow::Result<()> {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/nus-sky".into());
    std::fs::create_dir_all(&directory)?;
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
    let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))?;
    println!(
        "adapter={:?} timestamps={}",
        adapter.get_info(),
        !features.is_empty()
    );
    let init = Instant::now();
    let mut sky = SkyRenderer::new_offscreen(&device, &queue);
    println!(
        "renderer_init_cpu_ms={:.3}",
        init.elapsed().as_secs_f64() * 1000.0
    );
    println!(
        "pipeline_init_cpu_ms={:.3} noise_upload_cpu_ms={:.3}",
        sky.stats().init_pipeline_cpu_ms,
        sky.stats().init_noise_cpu_ms
    );
    let size = (1280, 820);
    let base = SkyParams::default();
    let solar = |hour: f32| {
        let a = (hour - 12.0) * std::f32::consts::PI / 12.0;
        [
            a.sin() * 0.78,
            ((hour - 6.0) * std::f32::consts::PI / 12.0).sin() * 0.95,
            0.64,
        ]
    };
    let cases = [
        ("broken", base),
        (
            "clear",
            SkyParams {
                low_cover: 0.12,
                high_cover: 0.55,
                ..base
            },
        ),
        (
            "overcast",
            SkyParams {
                low_cover: 0.89,
                stratus: 1.0,
                ..base
            },
        ),
        (
            "rain",
            SkyParams {
                low_cover: 0.89,
                stratus: 1.0,
                precipitation_mm_h: 6.0,
                ..base
            },
        ),
        (
            "sunset",
            SkyParams {
                sun_direction: solar(17.5),
                ..base
            },
        ),
        (
            "moon",
            SkyParams {
                sun_direction: solar(22.0),
                moon_direction: [-0.10, 0.55, 0.84],
                moon_illumination: 0.7,
                ..base
            },
        ),
    ];
    for (i, (name, mut p)) in cases.into_iter().enumerate() {
        p.time = i as f32;
        sky.render_offscreen(size, p);
        let timing = sky.read_gpu_timings()?;
        let image = capture(&device, &queue, sky.output_texture().unwrap())?;
        assert!(
            image.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
            "output must be opaque"
        );
        let (w, h) = sky.stats().output_size;
        std::fs::write(
            Path::new(&directory).join(format!("sky-{name}.png")),
            nus_render::icon::png(&image, w, h),
        )?;
        println!(
            "capture={name} volume_ms={:?} present_ms={:?}",
            timing.and_then(|x| x.0),
            timing.and_then(|x| x.1)
        );
    }
    let mut p = base;
    p.time = 10.0;
    sky.render_offscreen(size, p);
    sky.read_gpu_timings()?;
    let held = sky.stats();
    let held_bind = sky.render_offscreen(size, p);
    for _ in 0..48 {
        assert!(std::sync::Arc::ptr_eq(
            &held_bind,
            &sky.render_offscreen(size, p)
        ));
    }
    assert_eq!(sky.stats().volume_draws, held.volume_draws);
    assert_eq!(sky.stats().present_draws, held.present_draws);
    println!(
        "held_49_calls_gpu_draws=0 reuses={}",
        sky.stats().reuses - held.reuses
    );
    // A weather change while motion is held must update the volume immediately.
    let before_weather = sky.stats().volume_draws;
    sky.render_offscreen(
        size,
        SkyParams {
            low_cover: 0.89,
            stratus: 1.0,
            ..p
        },
    );
    sky.read_gpu_timings()?;
    assert_eq!(sky.stats().volume_draws, before_weather + 1);
    p.time = 11.0;
    sky.render_offscreen(size, p);
    sky.read_gpu_timings()?;
    let before_wind = sky.stats();
    p.wind_low = [45.0, 0.0];
    sky.render_offscreen(size, p);
    assert_eq!(
        sky.stats().present_draws,
        before_wind.present_draws,
        "wind changes at fixed animation time must not teleport cloud positions"
    );
    let mut volume = Vec::new();
    let mut present = Vec::new();
    let mut submit = Vec::new();
    let mut complete_volume = Vec::new();
    let mut complete_present = Vec::new();
    let mut zero_present = 0;
    for i in 1..=60 {
        p.time = 11.0 + i as f32 * 0.05;
        let before = sky.stats().volume_draws;
        let start = Instant::now();
        sky.render_offscreen(size, p);
        device.poll(wgpu::PollType::wait_indefinitely())?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if i > 8 {
            if sky.stats().volume_draws > before {
                complete_volume.push(elapsed);
            } else {
                complete_present.push(elapsed);
            }
            submit.push(sky.stats().cpu_submit_ms);
        }
        if let Some((v, pr)) = sky.read_gpu_timings()? {
            if i > 8 {
                if let Some(v) = v {
                    volume.push(v);
                }
                if let Some(pr) = pr {
                    present.push(pr);
                } else {
                    zero_present += 1;
                }
            }
        }
    }
    fn report(label: &str, values: &mut [f64]) {
        if values.is_empty() {
            println!("{label}=unavailable");
            return;
        }
        values.sort_by(f64::total_cmp);
        println!(
            "{label} n={} median_ms={:.4} p95_ms={:.4} max_ms={:.4}",
            values.len(),
            values[values.len() / 2],
            values[((values.len() - 1) as f64 * 0.95).round() as usize],
            values[values.len() - 1]
        );
    }
    report("volume_gpu", &mut volume);
    report("present_gpu", &mut present);
    report("submission_cpu", &mut submit);
    report("volume_submit_to_completion", &mut complete_volume);
    report("present_submit_to_completion", &mut complete_present);
    println!("unresolved_zero_present_timestamps={zero_present}; nonzero timestamp samples may be coalesced and are not a complete distribution");
    println!("stats={:?}", sky.stats());
    println!("Timings describe these GPU passes, not energy, display cadence, or whole-app cost.");
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
        label: Some("sky capture"),
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
