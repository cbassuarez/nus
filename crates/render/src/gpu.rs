//! wgpu device plus the one quad pipeline, and per-window `Target`s. Screen
//! size travels as an immediate, so bind groups are window-independent and
//! one `Gpu` can drive the main window, PiP, and the quick terminal.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use winit::window::Window;

use crate::scene::{Bind, Instance, Scene};
use crate::text::ATLAS_SIZE;

/// Device and immutable pipelines are shared by all main windows. Each
/// compositor still owns its glyph atlas and transient buffers: font atlas
/// coordinates belong to that window's FontSystem.
pub struct SharedGpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    instance: wgpu::Instance,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    hdr_pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    points_bgl: wgpu::BindGroupLayout,
    adapter: wgpu::Adapter,
}

thread_local! {
    // Weak ownership releases the device after the final window closes.
    static SHARED: std::cell::RefCell<std::sync::Weak<SharedGpu>> = Default::default();
}

pub struct Gpu {
    shared: Arc<SharedGpu>,
    atlas: wgpu::Texture,
    atlas_bind: wgpu::BindGroup,
    instances: wgpu::Buffer,
    instance_cap: usize,
    points: wgpu::Buffer,
    points_cap: usize,
    points_bind: wgpu::BindGroup,
}

impl std::ops::Deref for Gpu {
    type Target = SharedGpu;
    fn deref(&self) -> &SharedGpu {
        &self.shared
    }
}

/// A window's surface.
pub struct Target {
    surface: wgpu::Surface<'static>,
    /// Physical pixels.
    pub size: (u32, u32),
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    color_space: wgpu::SurfaceColorSpace,
    hdr_white_scale: f32,
    hdr_checked: std::time::Instant,
}

impl Target {
    pub fn hdr(&self) -> bool {
        self.color_space.is_hdr()
    }

    /// Whether the swapchain composites alpha, i.e. the window can be see-through.
    pub fn translucent(&self) -> bool {
        !matches!(
            self.alpha_mode,
            wgpu::CompositeAlphaMode::Opaque | wgpu::CompositeAlphaMode::Auto
        )
    }
}

impl Gpu {
    pub fn new(window: Arc<Window>) -> Result<(Gpu, Target)> {
        if let Some(shared) = SHARED.with(|s| s.borrow().upgrade()) {
            let surface = shared.instance.create_surface(window.clone())?;
            return Self::with_shared(shared, window, surface);
        }
        // Shared-texture import from CEF needs DX12 on Windows (D3D11 handles),
        // Metal on macOS (IOSurface) and Vulkan on Linux (dmabuf).
        let backends = if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else {
            wgpu::Backends::VULKAN
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| anyhow!("no adapter: {e:?}"))?;
        tracing::info!(
            "adapter: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::IMMEDIATES,
                required_limits: wgpu::Limits {
                    max_immediate_size: 16,
                    ..wgpu::Limits::default()
                },
                ..Default::default()
            }))?;
        let format = wgpu::TextureFormat::Bgra8Unorm;

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let points_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("points bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad pl"),
            bind_group_layouts: &[Some(&bgl), Some(&points_bgl)],
            immediate_size: 16,
        });
        let make_pipeline = |format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Uint32, 5 => Uint32, 6 => Float32, 7 => Uint32
                    ],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
        };
        let pipeline = make_pipeline(format);
        let hdr_pipeline = make_pipeline(wgpu::TextureFormat::Rgba16Float);
        let shared = Arc::new(SharedGpu {
            device,
            queue,
            instance,
            format,
            pipeline,
            hdr_pipeline,
            bgl,
            sampler,
            points_bgl,
            adapter,
        });
        SHARED.with(|s| *s.borrow_mut() = Arc::downgrade(&shared));
        Self::with_shared(shared, window, surface)
    }

    fn with_shared(
        shared: Arc<SharedGpu>,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
    ) -> Result<(Gpu, Target)> {
        let SharedGpu {
            device,
            bgl,
            sampler,
            points_bgl,
            format,
            ..
        } = shared.as_ref();
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            // COPY_SRC so the app can photograph its own glyph atlas.
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_bind = Self::make_bind(device, bgl, &atlas_view, sampler);

        let points_cap = 4096;
        let points = Self::make_points(device, points_cap);
        let points_bind = Self::bind_points(device, points_bgl, &points);
        let instance_cap = 8192;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (std::mem::size_of::<Instance>() * instance_cap) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let size = window.inner_size();
        let format = *format;
        let gpu = Gpu {
            shared,
            atlas,
            atlas_bind,
            instances,
            instance_cap,
            points,
            points_cap,
            points_bind,
        };
        let mut target = Target {
            surface,
            size: (size.width.max(1), size.height.max(1)),
            format,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            color_space: wgpu::SurfaceColorSpace::Auto,
            hdr_white_scale: 1.0,
            hdr_checked: std::time::Instant::now(),
        };
        target.select_color_space(&gpu.adapter);
        target.alpha_mode = gpu.alpha_mode_for(&target.surface);
        target.configure(&gpu.device);
        Ok((gpu, target))
    }

    /// Premultiplied alpha when the compositor offers it, so a window made
    /// transparent can show the desktop through its paper.
    fn alpha_mode_for(&self, surface: &wgpu::Surface<'static>) -> wgpu::CompositeAlphaMode {
        let caps = surface.get_capabilities(&self.adapter);
        let mode = if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            wgpu::CompositeAlphaMode::Auto
        };
        tracing::info!("surface alpha modes {:?} → {:?}", caps.alpha_modes, mode);
        mode
    }

    /// A surface for another window (PiP, quick terminal).
    pub fn target(&self, window: Arc<Window>) -> Result<Target> {
        let surface = self.instance.create_surface(window.clone())?;
        let size = window.inner_size();
        let alpha_mode = self.alpha_mode_for(&surface);
        let mut t = Target {
            surface,
            size: (size.width.max(1), size.height.max(1)),
            format: self.format,
            alpha_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            hdr_white_scale: 1.0,
            hdr_checked: std::time::Instant::now(),
        };
        t.select_color_space(&self.adapter);
        t.configure(&self.device);
        Ok(t)
    }

    fn make_points(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("points"),
            size: (std::mem::size_of::<[f32; 2]>() * cap) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn bind_points(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        points: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("points"),
            layout: bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: points.as_entire_binding(),
            }],
        })
    }

    fn make_bind(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// A bind group for an external RGBA texture (e.g. a CEF paint).
    pub fn bind_texture(&self, texture: &wgpu::Texture) -> Arc<wgpu::BindGroup> {
        self.texture_binder().bind(texture)
    }

    pub fn texture_binder(&self) -> TextureBinder {
        TextureBinder {
            device: self.device.clone(),
            bgl: self.bgl.clone(),
            sampler: self.sampler.clone(),
        }
    }

    pub fn upload_glyph(&self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draw a scene into `target`. Returns false if the surface wasn't available.
    pub fn render(&mut self, target: &mut Target, scene: &Scene, clear: [f32; 4]) -> bool {
        // scRGB uses an absolute 80-nit unit on Windows. Track the user's SDR
        // white setting when moving displays, without polling OS APIs per frame.
        if cfg!(target_os = "windows")
            && target.hdr()
            && target.hdr_checked.elapsed().as_secs_f32() > 1.0
        {
            target.update_white_scale(&self.adapter);
        }
        self.upload_instances(scene);
        let frame = match target.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                target.configure(&self.device);
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                // A native resize/show can invalidate the drawable before a
                // resize callback arrives. Reconfigure and retry next frame.
                target.configure(&self.device);
                return false;
            }
            _ => return false,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.pass(
            &mut encoder,
            &view,
            target.size,
            scene,
            clear,
            if target.hdr() {
                target.hdr_white_scale
            } else {
                0.0
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(frame);
        true
    }

    /// The scene into an offscreen texture, read back as RGBA8 rows: the
    /// app photographing itself, from its own texture rather than the OS.
    /// The glyph atlas as it sits on the GPU: one byte of coverage per
    /// pixel, `ATLAS_SIZE` square. Every letter the app has drawn so far.
    pub fn atlas_snapshot(&mut self) -> (u32, Vec<u8>) {
        let size = ATLAS_SIZE;
        let row = size.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("atlas readback"),
            size: (row * size) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("atlas"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size),
                },
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let _ = rx.recv();
        let data = slice.get_mapped_range().expect("atlas readback mapped");
        let mut out = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            let start = (y * row) as usize;
            out.extend_from_slice(&data[start..start + size as usize]);
        }
        drop(data);
        buffer.unmap();
        (size, out)
    }

    pub fn snapshot(&mut self, size: (u32, u32), scene: &Scene, clear: [f32; 4]) -> Vec<u8> {
        self.snapshot_pixels(size, scene, clear, false)
    }

    /// Straight-alpha capture for transparent window choreography. Ordinary
    /// document/page snapshots retain their existing opaque output.
    pub fn snapshot_alpha(&mut self, size: (u32, u32), scene: &Scene, clear: [f32; 4]) -> Vec<u8> {
        self.snapshot_pixels(size, scene, clear, true)
    }

    /// Native capture regression: read the actual radiance shader's float16
    /// output. PNG screenshots cannot establish that a highlight exceeds SDR.
    pub fn verify_hdr_signal(&mut self) -> Result<f32> {
        let size = (128, 24);
        let mut scene = Scene::new();
        scene.push(Instance::loading_light(
            crate::Rect::new(0.0, 0.0, 128.0, 16.0),
            0.5,
            1.0,
            [1.0; 4],
        ));
        scene.rect(crate::Rect::new(0.0, 20.0, 8.0, 4.0), [1.0; 4]);
        scene.finish();
        self.upload_instances(&scene);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HDR radiance verification"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride = size.0 * 8;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HDR radiance readback"),
            size: (stride * size.1) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.pass(
            &mut encoder,
            &texture.create_view(&Default::default()),
            size,
            &scene,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size.1),
                },
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?;
        let reference = u16::from_le_bytes([
            data[(20 * stride) as usize],
            data[(20 * stride) as usize + 1],
        ]);
        let peak = data
            .chunks_exact(8)
            .flat_map(|px| {
                [
                    u16::from_le_bytes([px[0], px[1]]),
                    u16::from_le_bytes([px[2], px[3]]),
                    u16::from_le_bytes([px[4], px[5]]),
                ]
            })
            .max()
            .unwrap_or(0);
        if reference != 0x3c00 || !(0x4000..0x4500).contains(&peak) {
            return Err(anyhow!(
                "HDR signal/reference mismatch: {peak:04x}/{reference:04x}"
            ));
        }
        let value =
            2.0_f32.powi(((peak >> 10) & 31) as i32 - 15) * (1.0 + (peak & 1023) as f32 / 1024.0);
        drop(data);
        buffer.unmap();
        Ok(value)
    }

    fn snapshot_pixels(
        &mut self,
        size: (u32, u32),
        scene: &Scene,
        clear: [f32; 4],
        alpha: bool,
    ) -> Vec<u8> {
        self.upload_instances(scene);
        let (w, h) = size;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("snapshot"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Rows are padded to 256 bytes for the copy.
        let stride = (w * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snapshot readback"),
            size: (stride * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.pass(&mut encoder, &view, size, scene, clear, 0.0);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let _ = rx.recv();
        let data = slice.get_mapped_range().expect("snapshot readback mapped");
        let bgra = matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let r = &data[(row * stride) as usize..(row * stride + w * 4) as usize];
            for px in r.chunks(4) {
                let a = if alpha { px[3] } else { 255 };
                let channel = |c: u8| {
                    if alpha && a > 0 {
                        ((c as u32 * 255) / a as u32).min(255) as u8
                    } else {
                        c
                    }
                };
                if bgra {
                    out.extend_from_slice(&[channel(px[2]), channel(px[1]), channel(px[0]), a]);
                } else {
                    out.extend_from_slice(&[channel(px[0]), channel(px[1]), channel(px[2]), a]);
                }
            }
        }
        drop(data);
        buffer.unmap();
        out
    }

    fn upload_instances(&mut self, scene: &Scene) {
        let all = scene.instances();
        if all.len() > self.instance_cap {
            self.instance_cap = all.len().next_power_of_two();
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: (std::mem::size_of::<Instance>() * self.instance_cap) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !all.is_empty() {
            self.queue
                .write_buffer(&self.instances, 0, bytemuck::cast_slice(all));
        }
        let pts = scene.points();
        if pts.len() > self.points_cap {
            self.points_cap = pts.len().next_power_of_two();
            self.points = Self::make_points(&self.device, self.points_cap);
            self.points_bind = Self::bind_points(&self.device, &self.points_bgl, &self.points);
        }
        if !pts.is_empty() {
            self.queue
                .write_buffer(&self.points, 0, bytemuck::cast_slice(pts));
        }
    }

    /// One render pass of `scene` into `view`, cleared to `clear`.
    fn pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: (u32, u32),
        scene: &Scene,
        clear: [f32; 4],
        hdr_scale: f32,
    ) {
        let hdr = hdr_scale > 0.0;
        let clear = if hdr {
            [
                srgb_linear(clear[0]) * hdr_scale,
                srgb_linear(clear[1]) * hdr_scale,
                srgb_linear(clear[2]) * hdr_scale,
                clear[3],
            ]
        } else {
            clear
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: clear[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(if hdr {
                &self.hdr_pipeline
            } else {
                &self.pipeline
            });
            let (sw, sh) = size;
            pass.set_immediates(
                0,
                bytemuck::cast_slice(&[sw as f32, sh as f32, scene.corner_radius, hdr_scale]),
            );
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.set_bind_group(1, &self.points_bind, &[]);
            for layer in scene.layers() {
                if layer.range.is_empty() {
                    continue;
                }
                let (x, y, w, h) = match layer.clip {
                    Some(c) => {
                        let x = c.x.max(0.0).min(sw as f32) as u32;
                        let y = c.y.max(0.0).min(sh as f32) as u32;
                        let w = (c.x + c.w).min(sw as f32).max(x as f32) as u32 - x;
                        let h = (c.y + c.h).min(sh as f32).max(y as f32) as u32 - y;
                        (x, y, w, h)
                    }
                    None => (0, 0, sw, sh),
                };
                if w == 0 || h == 0 {
                    continue;
                }
                pass.set_scissor_rect(x, y, w, h);
                match &layer.bind {
                    Bind::Atlas => pass.set_bind_group(0, &self.atlas_bind, &[]),
                    Bind::External(bg) => pass.set_bind_group(0, bg.as_ref(), &[]),
                }
                pass.draw(0..6, layer.range.start as u32..layer.range.end as u32);
            }
        }
    }
}

impl Target {
    fn update_white_scale(&mut self, adapter: &wgpu::Adapter) {
        self.hdr_checked = std::time::Instant::now();
        if cfg!(target_os = "windows") {
            self.hdr_white_scale = self
                .surface
                .display_hdr_info(adapter)
                .luminance
                .and_then(|l| l.sdr_white_nits)
                .filter(|n| n.is_finite() && *n > 0.0)
                .map(|n| (n / 80.0).clamp(0.25, 12.5))
                .unwrap_or(1.0);
        }
    }
    fn select_color_space(&mut self, adapter: &wgpu::Adapter) {
        let caps = self.surface.get_capabilities(adapter);
        if caps
            .color_spaces(wgpu::TextureFormat::Rgba16Float)
            .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)
        {
            self.format = wgpu::TextureFormat::Rgba16Float;
            self.color_space = wgpu::SurfaceColorSpace::ExtendedSrgbLinear;
            self.update_white_scale(adapter);
        }
        tracing::info!(
            "display output: {:?} {:?}; {:?}",
            self.format,
            self.color_space,
            self.surface.display_hdr_info(adapter)
        );
    }

    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        // Windows reports transient nonsense (0, or 32767) mid-move; the
        // swapchain must stay within the device's texture limit.
        let max = device.limits().max_texture_dimension_2d;
        if w == 0 || h == 0 || w > max || h > max || (w, h) == self.size {
            return;
        }
        self.size = (w, h);
        self.configure(device);
    }

    fn configure(&mut self, device: &wgpu::Device) {
        self.surface.configure(
            device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.format,
                color_space: self.color_space,
                view_formats: vec![self.format],
                alpha_mode: self.alpha_mode,
                width: self.size.0,
                height: self.size.1,
                desired_maximum_frame_latency: 1,
                present_mode: wgpu::PresentMode::AutoVsync,
            },
        );
    }
}

fn srgb_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// A cloneable handle that can bind external textures without borrowing
/// the whole [`Gpu`] — CEF paint callbacks hold one.
#[derive(Clone)]
pub struct TextureBinder {
    device: wgpu::Device,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl TextureBinder {
    pub fn bind(&self, texture: &wgpu::Texture) -> Arc<wgpu::BindGroup> {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Arc::new(Gpu::make_bind(
            &self.device,
            &self.bgl,
            &view,
            &self.sampler,
        ))
    }
}
