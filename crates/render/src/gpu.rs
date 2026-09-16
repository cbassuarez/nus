//! wgpu device plus the one quad pipeline, and per-window `Target`s. Screen
//! size travels as an immediate, so bind groups are window-independent and
//! one `Gpu` can drive the main window, PiP, and the quick terminal.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use winit::window::Window;

use crate::scene::{Bind, Instance, Scene};
use crate::text::ATLAS_SIZE;

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    instance: wgpu::Instance,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    atlas: wgpu::Texture,
    atlas_bind: wgpu::BindGroup,
    instances: wgpu::Buffer,
    instance_cap: usize,
    adapter: wgpu::Adapter,
}

/// A window's surface.
pub struct Target {
    surface: wgpu::Surface<'static>,
    /// Physical pixels.
    pub size: (u32, u32),
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
}

impl Target {
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
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
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
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_bind = Self::make_bind(&device, &bgl, &atlas_view, &sampler);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 8,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Uint32, 5 => Uint32, 6 => Float32
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
        });
        let instance_cap = 8192;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (std::mem::size_of::<Instance>() * instance_cap) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let size = window.inner_size();
        let gpu = Gpu {
            device,
            queue,
            instance,
            format,
            pipeline,
            bgl,
            sampler,
            atlas,
            atlas_bind,
            instances,
            instance_cap,
            adapter,
        };
        let mut target = Target {
            surface,
            size: (size.width.max(1), size.height.max(1)),
            format,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
        };
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
        };
        t.configure(&self.device);
        Ok(t)
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
        let frame = match target.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                target.configure(&self.device);
                f
            }
            _ => return false,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pass.set_pipeline(&self.pipeline);
            let (sw, sh) = target.size;
            pass.set_immediates(0, bytemuck::cast_slice(&[sw as f32, sh as f32]));
            pass.set_vertex_buffer(0, self.instances.slice(..));
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
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(frame);
        true
    }
}

impl Target {
    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if w == 0 || h == 0 || (w, h) == self.size {
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
                color_space: wgpu::SurfaceColorSpace::Auto,
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
