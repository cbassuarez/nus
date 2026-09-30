//! The production Scene encoder + quad shader, without CEF or a window.
//! Unit/ABI tests live beside Scene; the real GPU readback is explicit opt-in.
use nus_render::{Bind, Instance, Rect, Scene};
use wgpu::util::DeviceExt;

#[test]
fn reading_overlay_production_shader_validates() {
    let source = include_str!("../src/quad.wgsl");
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
}

#[test]
#[ignore = "requires a native IMMEDIATES-capable GPU adapter; run with --ignored --nocapture"]
fn reading_overlay_sdr_and_hdr_gpu_readback() {
    pollster::block_on(gpu_readback()).expect("reading overlay GPU readback");
}

async fn gpu_readback() -> anyhow::Result<()> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await?;
    eprintln!("reading overlay adapter: {:?}", adapter.get_info());
    let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::IMMEDIATES,
        required_limits: wgpu::Limits { max_immediate_size: 32, ..Default::default() },
        ..Default::default()
    }).await?;

    let mut scene = Scene::new();
    scene.layer(Some(Rect::new(0.0, 0.0, 50.0, 64.0)));
    scene.reading_fields_weighted(
        [(Rect::new(8.0, 8.0, 24.0, 24.0), 4.0, 1.0),
         (Rect::new(20.0, 26.0, 28.0, 8.0), 4.0, 0.3)],
        [0.0, 0.0, 0.0, 0.10],
    );
    // Old callers still passing an HDR fallback must not get opaque paper.
    scene.reading_fields(
        [(Rect::new(8.0, 44.0, 8.0, 8.0), 0.0),
         (Rect::new(0.0, 0.0, 0.0, 0.0), 0.0)],
        [0.0, 0.0, 0.0, 0.10],
        Some([0.293372, 0.293372, 0.293372, 1.0]),
    );
    scene.layer(None);
    scene.rect(Rect::new(54.0, 4.0, 6.0, 6.0), [0.75, 0.20, 0.10, 1.0]);
    scene.finish();

    let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("overlay test atlas layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true } }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ],
    });
    let point_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("overlay test points layout"),
        entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false, min_binding_size: None }, count: None }],
    });
    let atlas = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("overlay test unused atlas"),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
    });
    let atlas_view = atlas.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let atlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("overlay test atlas"), layout: &tex_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&atlas_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });
    let points = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("overlay test encoded points"), contents: bytemuck::cast_slice(scene.points()),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let point_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("overlay test point bind"), layout: &point_bgl,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: points.as_entire_binding() }],
    });
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("overlay test production instances"),
        contents: bytemuck::cast_slice(scene.instances()), usage: wgpu::BufferUsages::VERTEX,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production quad.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../src/quad.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("overlay test pipeline layout"),
        bind_group_layouts: &[Some(&tex_bgl), Some(&point_bgl)], immediate_size: 32,
    });

    for (format, hdr_scale, bytes_per_pixel) in [
        (wgpu::TextureFormat::Rgba8Unorm, 0.0_f32, 4_u32),
        (wgpu::TextureFormat::Rgba16Float, 1.0_f32, 8_u32),
        (wgpu::TextureFormat::Rgba16Float, 2.0_f32, 8_u32),
    ] {
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("reading overlay readback"), layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader, entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4,
                        4 => Uint32, 5 => Uint32, 6 => Float32, 7 => Uint32
                    ],
                })], compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default() }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            multiview_mask: None, cache: None,
        });
        for transparent in [false, true] {
            let white = if transparent { 0.0 } else { hdr_scale.max(1.0) };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("overlay readback target"),
                size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let stride = 64 * bytes_per_pixel; // 256/512 bytes: both row-aligned.
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay readback buffer"), size: u64::from(stride * 64),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let view = texture.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("overlay readback pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view, resolve_target: None, depth_slice: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(white), g: f64::from(white), b: f64::from(white),
                            a: if transparent { 0.0 } else { 1.0 },
                        }), store: wgpu::StoreOp::Store },
                    })], ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.set_immediates(0, bytemuck::cast_slice(&[64.0_f32,64.0,0.0,hdr_scale,3.0,0.0,0.0,0.0]));
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_bind_group(0, &atlas_bind, &[]);
                pass.set_bind_group(1, &point_bind, &[]);
                for layer in scene.layers() {
                    assert!(matches!(&layer.bind, Bind::Atlas));
                    let clip = layer.clip.unwrap_or(Rect::new(0.0,0.0,64.0,64.0));
                    pass.set_scissor_rect(clip.x as u32,clip.y as u32,clip.w as u32,clip.h as u32);
                    pass.draw(0..6, layer.range.start as u32..layer.range.end as u32);
                }
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0,
                    origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout {
                    offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(64) } },
                wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
            );
            queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
            device.poll(wgpu::PollType::wait_indefinitely())?;
            rx.recv()??;
            let bytes = slice.get_mapped_range()?;
            let pixel = |x: usize, y: usize| -> [f32; 4] {
                let at = y * stride as usize + x * bytes_per_pixel as usize;
                std::array::from_fn(|c| {
                    if bytes_per_pixel == 4 { f32::from(bytes[at+c]) / 255.0 }
                    else { half(u16::from_le_bytes([bytes[at+c*2], bytes[at+c*2+1]])) }
                })
            };
            // Expected alpha at: core, footer, overlap, outer feather, clipped
            // feather, untouched background, and legacy Some(surface) caller.
            for (name,x,y,alpha) in [
                ("reading",12,12,0.10), ("footer",44,30,0.03),
                ("overlap",24,28,0.10), ("reading feather",5,16,0.031640625),
                ("clipped footer feather",50,30,0.0), ("outside",55,55,0.0),
                ("legacy HDR paper removed",10,46,0.10),
            ] {
                let got = pixel(x,y);
                let expected_rgb = white * (1.0-alpha);
                let expected_alpha = if transparent { alpha } else { 1.0 };
                for channel in &got[..3] {
                    assert!((*channel-expected_rgb).abs()<0.008,
                        "{name}: {format:?} scale={hdr_scale} transparent={transparent}: {got:?}");
                }
                assert!((got[3]-expected_alpha).abs()<0.008, "{name}: alpha {got:?}");
            }
            let solid = pixel(56,6);
            for (got, source) in solid[..3].iter().zip([0.75_f32,0.20,0.10]) {
                let expected = if hdr_scale > 0.0 { linear(source)*hdr_scale } else { source };
                assert!((*got-expected).abs()<0.008, "ordinary opaque material changed: {solid:?}");
            }
            assert!((solid[3]-1.0).abs()<0.008);
            drop(bytes);
            readback.unmap();
        }
    }
    Ok(())
}

fn linear(value: f32) -> f32 {
    if value <= 0.04045 { value/12.92 } else { ((value+0.055)/1.055).powf(2.4) }
}

// Minimal IEEE-754 binary16 decoder, avoiding a new test dependency.
fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = (bits >> 10) & 31;
    let mantissa = f32::from(bits & 1023);
    if exponent == 0 { sign * 2.0_f32.powi(-14) * mantissa/1024.0 }
    else if exponent == 31 { if mantissa == 0.0 { sign*f32::INFINITY } else { f32::NAN } }
    else { sign * 2.0_f32.powi(i32::from(exponent)-15) * (1.0+mantissa/1024.0) }
}

#[test]
fn reading_overlay_half_decoder_known_values() {
    assert_eq!(half(0x0000),0.0);
    assert_eq!(half(0x3c00),1.0);
    assert_eq!(half(0x4000),2.0);
    assert_eq!(half(0xbc00),-1.0);
    assert_eq!(half(0x0001),2.0_f32.powi(-24));
}
