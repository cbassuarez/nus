//! Recovered Limb / Darkroom artwork, rendered by wgpu, without a browser.
//! Image maps, star buffers and pipelines are shared; each view owns only its
//! uniform, bounded output and cached dust target. Identical frames submit nothing.
use crate::{Gpu, TextureBinder};
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;
use wgpu::util::DeviceExt;
#[path = "space_catalog.rs"] mod catalog;

pub const OUTPUT_MAX: u32 = 1200;
pub const DUST_MAX: u32 = 720;
pub const PLANET_RADIUS: f32 = 2.67939;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceParams {
    pub phase: f32,
    pub blend: f32,
    pub exposure: f32,
    pub time: f32,
    pub seed: u32,
    pub lines: bool,
    /// Top-left normalized geometry. Attenuates stars only, not artwork/overlay.
    pub reading_rect: [f32; 4],
}
impl Default for SpaceParams {
    fn default() -> Self {
        Self { phase: 0.5 + 0.46 * 0.30_f32.cos(), blend: 0.0,
            exposure: 1.0, time: 0.0, seed: 42, lines: true, reading_rect: [0.0; 4] }
    }
}
impl SpaceParams {
    pub fn clean(mut self) -> Self {
        let d = Self::default();
        self.phase = finite(self.phase, d.phase).clamp(0.0, 1.0);
        self.blend = finite(self.blend, d.blend).clamp(0.0, 1.0);
        self.exposure = finite(self.exposure, 1.0).clamp(0.05, 3.0);
        self.time = finite(self.time, 0.0).max(0.0);
        self.seed = self.seed.min(1_000_000);
        for v in &mut self.reading_rect { *v = finite(*v, 0.0).clamp(0.0, 1.0); }
        self
    }
    pub fn planet_center(self) -> [f32; 3] {
        let ph = self.phase - 0.5;
        [1.18364 + ph * 0.12 + self.blend * 0.70, -2.40377 + ph * 0.06 - self.blend * 0.75, -1.0]
    }
}
/// Decoding stays in the caller, which already has the image codec dependency.
/// Pixels must be straight RGBA8 in the JPEG's original top-to-bottom order.
pub struct SpaceImage { pub width: u32, pub height: u32, pub rgba: Vec<u8> }
impl SpaceImage {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!((1..=4096).contains(&self.width) && (1..=4096).contains(&self.height), "Space image dimensions out of bounds");
        let n = self.width as usize * self.height as usize * 4;
        anyhow::ensure!(self.rgba.len() == n, "Space image RGBA byte count mismatch");
        Ok(())
    }
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms { resolution: [f32; 4], view: [f32; 4], options: [f32; 4], reading: [f32; 4] }
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Star { direction: [f32; 3], magnitude: f32, bv: f32 }
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Segment { a: [f32; 3], b: [f32; 3] }
const STAR_ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32];
const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

/// Shared by the Home and Settings views within one GPU/window.
pub struct SpaceResources {
    device: wgpu::Device,
    queue: wgpu::Queue,
    binder: Option<TextureBinder>,
    standalone_bgl: wgpu::BindGroupLayout,
    dust_bgl: wgpu::BindGroupLayout,
    scene_bgl: wgpu::BindGroupLayout,
    dust_pipeline: wgpu::RenderPipeline,
    scene_pipeline: wgpu::RenderPipeline,
    star_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    maps: Vec<wgpu::Texture>,
    map_sampler: wgpu::Sampler,
    cloud_sampler: wgpu::Sampler,
    dust_sampler: wgpu::Sampler,
    stars: wgpu::Buffer,
    segments: wgpu::Buffer,
    star_count: u32,
    segment_count: u32,
    pub image_bytes: u64,
}
impl SpaceResources {
    pub fn new(gpu: &Gpu, images: [SpaceImage; 4]) -> anyhow::Result<Arc<Self>> {
        Self::build(&gpu.device, &gpu.queue, Some(gpu.texture_binder()), images)
    }
    /// Native GPU harness: no window, no platform browser, no Scene bind layout.
    pub fn new_offscreen(device: &wgpu::Device, queue: &wgpu::Queue, images: [SpaceImage; 4]) -> anyhow::Result<Arc<Self>> {
        Self::build(device, queue, None, images)
    }
    fn build(device: &wgpu::Device, queue: &wgpu::Queue, binder: Option<TextureBinder>, images: [SpaceImage; 4]) -> anyhow::Result<Arc<Self>> {
        for image in &images {
            image.validate()?;
            anyhow::ensure!(image.width.max(image.height) <= device.limits().max_texture_dimension_2d, "Space image exceeds this adapter's texture limit");
        }
        let decoded = decode_stars(include_bytes!("space-data/stars.bin"))?;
        let mut segments = Vec::with_capacity(catalog::LINES.len());
        for [a, b] in catalog::LINES {
            let a = decoded.get(*a as usize).ok_or_else(|| anyhow::anyhow!("Space figure index out of range"))?;
            let b = decoded.get(*b as usize).ok_or_else(|| anyhow::anyhow!("Space figure index out of range"))?;
            segments.push(Segment { a: a.direction, b: b.direction });
        }
        let uniform_entry = wgpu::BindGroupLayoutEntry { binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64) }, count: None };
        let dust_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("Space dust layout"), entries: &[uniform_entry] });
        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Space scene layout"), entries: &[uniform_entry, texture_entry(1), texture_entry(2), texture_entry(3), texture_entry(4), texture_entry(5), sampler_entry(6), sampler_entry(7), sampler_entry(8)] });
        let standalone_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Space standalone output"), entries: &[texture_entry(0), sampler_entry(1)] });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("Space Limb / Darkroom"),
            source: wgpu::ShaderSource::Wgsl(include_str!("space.wgsl").into()) });
        let dust_pipeline = pipeline(device, &shader, &dust_bgl, "Space dust", "vs_main", "dust_main", &[], None);
        let scene_pipeline = pipeline(device, &shader, &scene_bgl, "Space Earth and atmosphere", "vs_main", "scene_main", &[], None);
        let edge_pipeline = pipeline(device, &shader, &scene_bgl, "Space authored edge shade", "vs_main", "edge_fs", &[], Some(wgpu::BlendState::ALPHA_BLENDING));
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            // Preserve the opaque base; catalogue fragments carry zero alpha.
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let star_pipeline = pipeline(device, &shader, &scene_bgl, "Space catalogue stars", "star_vs", "star_fs",
            &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Star>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &STAR_ATTRIBUTES })], Some(additive));
        let line_pipeline = pipeline(device, &shader, &scene_bgl, "Space constellation figures", "line_vs", "line_fs",
            &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Segment>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &LINE_ATTRIBUTES })], Some(additive));
        let stars = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("Space Hipparcos catalogue"), contents: bytemuck::cast_slice(&decoded), usage: wgpu::BufferUsages::VERTEX });
        let segments_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("Space Western figures"), contents: bytemuck::cast_slice(&segments), usage: wgpu::BufferUsages::VERTEX });
        let map_sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("Space image mipmaps"),
            mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear, anisotropy_clamp: 8, ..Default::default() });
        let cloud_sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("Space cloud mipmaps"),
            mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear, anisotropy_clamp: 4, ..Default::default() });
        let dust_sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("Space dust cache"),
            mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let mut maps = Vec::with_capacity(4); let mut image_bytes = 0;
        for image in images { let (texture, bytes) = upload_map(device, queue, image); maps.push(texture); image_bytes += bytes; }
        Ok(Arc::new(Self { device: device.clone(), queue: queue.clone(), binder, standalone_bgl, dust_bgl, scene_bgl,
            dust_pipeline, scene_pipeline, star_pipeline, edge_pipeline, line_pipeline, maps, map_sampler, cloud_sampler, dust_sampler,
            stars, segments: segments_buffer, star_count: decoded.len() as u32, segment_count: segments.len() as u32, image_bytes }))
    }
}
#[derive(Debug, Default, Clone, Copy)]
pub struct SpaceStats {
    pub dust_draws: u64,
    pub scene_draws: u64,
    pub reuses: u64,
    pub output_size: (u32, u32),
    pub dust_size: (u32, u32),
    /// Per-view targets only. Shared images are accounted by SpaceResources.
    pub target_bytes: u64,
}
struct Targets {
    _dust: wgpu::Texture,
    dust_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    scene_bind: Arc<wgpu::BindGroup>,
}
pub struct SpaceRenderer {
    resources: Arc<SpaceResources>,
    uniform: wgpu::Buffer,
    dust_bind: wgpu::BindGroup,
    targets: Option<Targets>,
    previous: Option<SpaceParams>,
    stats: SpaceStats,
}
impl SpaceRenderer {
    pub fn new(resources: Arc<SpaceResources>) -> Self {
        let uniform = resources.device.create_buffer(&wgpu::BufferDescriptor { label: Some("Space view uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let dust_bind = resources.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("Space dust uniforms"), layout: &resources.dust_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }] });
        Self { resources, uniform, dust_bind, targets: None, previous: None, stats: SpaceStats::default() }
    }
    pub fn stats(&self) -> SpaceStats { self.stats }
    pub fn output_texture(&self) -> Option<&wgpu::Texture> { self.targets.as_ref().map(|t| &t.output) }
    pub fn render(&mut self, size: (u32, u32), params: SpaceParams) -> Arc<wgpu::BindGroup> {
        let p = params.clean();
        let cap = OUTPUT_MAX.min(self.resources.device.limits().max_texture_dimension_2d).max(1);
        let output_size = bounded(size, cap); let dust_size = bounded(output_size, DUST_MAX.min(cap));
        let resized = self.targets.is_none() || output_size != self.stats.output_size;
        if !resized && self.previous == Some(p) { self.stats.reuses += 1; return self.targets.as_ref().unwrap().scene_bind.clone(); }
        if resized { self.make_targets(output_size, dust_size); }
        let dust_dirty = resized || self.previous.is_none_or(|old| old.seed != p.seed);
        let u = Uniforms { resolution: [output_size.0 as f32, output_size.1 as f32, dust_size.0 as f32, dust_size.1 as f32],
            view: [p.phase, p.blend, p.exposure, p.time], options: [p.seed as f32, if p.lines {1.0} else {0.0}, 0.0, 0.0], reading: p.reading_rect };
        let res = &self.resources;
        res.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        let mut encoder = res.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Space frame") });
        let t = self.targets.as_ref().unwrap();
        if dust_dirty {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("Space dust cache"),
                color_attachments: &[Some(attachment(&t.dust_view))], ..Default::default() });
            pass.set_pipeline(&res.dust_pipeline); pass.set_bind_group(0, &self.dust_bind, &[]); pass.draw(0..3, 0..1);
            self.stats.dust_draws += 1;
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("Space scene and catalogue"),
                color_attachments: &[Some(attachment(&t.output_view))], ..Default::default() });
            pass.set_bind_group(0, &t.bind, &[]); pass.set_pipeline(&res.scene_pipeline); pass.draw(0..3, 0..1);
            if p.lines { pass.set_pipeline(&res.line_pipeline); pass.set_vertex_buffer(0, res.segments.slice(..)); pass.draw(0..6, 0..res.segment_count); }
            pass.set_pipeline(&res.star_pipeline); pass.set_vertex_buffer(0, res.stars.slice(..)); pass.draw(0..6, 0..res.star_count);
            pass.set_pipeline(&res.edge_pipeline); pass.draw(0..3,0..1);
        }
        res.queue.submit([encoder.finish()]); self.stats.scene_draws += 1; self.previous = Some(p);
        t.scene_bind.clone()
    }
    fn make_targets(&mut self, output_size: (u32, u32), dust_size: (u32, u32)) {
        let r = &self.resources;
        let dust = target(&r.device, "Space cached dust", dust_size);
        let output = target(&r.device, "Space output", output_size);
        let dust_view = dust.create_view(&Default::default()); let output_view = output.create_view(&Default::default());
        let views: Vec<_> = r.maps.iter().map(|t| t.create_view(&Default::default())).collect();
        let bind = r.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("Space view maps"), layout: &r.scene_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                texture_binding(1, &dust_view), texture_binding(2, &views[0]), texture_binding(3, &views[1]),
                texture_binding(4, &views[2]), texture_binding(5, &views[3]),
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&r.map_sampler) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&r.dust_sampler) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&r.cloud_sampler) }] });
        let scene_bind = r.binder.as_ref().map(|b| b.bind(&output)).unwrap_or_else(|| Arc::new(r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Space standalone bind"), layout: &r.standalone_bgl,
            entries: &[texture_binding(0, &output_view), wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&r.dust_sampler) }] })));
        self.targets = Some(Targets { _dust: dust, dust_view, output, output_view, bind, scene_bind });
        self.stats.output_size = output_size; self.stats.dust_size = dust_size;
        self.stats.target_bytes = (u64::from(output_size.0)*u64::from(output_size.1) + u64::from(dust_size.0)*u64::from(dust_size.1))*4;
    }
}
#[allow(clippy::too_many_arguments)]
fn pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, bgl: &wgpu::BindGroupLayout,
    label: &str, vertex: &str, fragment: &str, buffers: &[Option<wgpu::VertexBufferLayout<'_>>], blend: Option<wgpu::BlendState>) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[Some(bgl)], immediate_size: 0 });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some(label), layout: Some(&layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some(vertex), buffers, compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }
}
fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }
}
fn texture_binding(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) }
}
fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment { view, resolve_target: None, depth_slice: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } }
}
fn target(device: &wgpu::Device, label: &str, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: extent(size), mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[] })
}
fn extent(size: (u32, u32)) -> wgpu::Extent3d { wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 } }
fn upload_map(device: &wgpu::Device, queue: &wgpu::Queue, image: SpaceImage) -> (wgpu::Texture, u64) {
    let levels = 32 - image.width.max(image.height).leading_zeros();
    let texture = device.create_texture(&wgpu::TextureDescriptor { label: Some("Space historical image"), size: extent((image.width, image.height)),
        mip_level_count: levels, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    let (mut w, mut h, mut pixels) = (image.width, image.height, image.rgba); let mut bytes = 0;
    for level in 0..levels {
        queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &pixels, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w*4), rows_per_image: Some(h) }, extent((w,h)));
        bytes += pixels.len() as u64;
        if level + 1 < levels { let next = downsample(&pixels,w,h); w=(w/2).max(1); h=(h/2).max(1); pixels=next; }
    }
    (texture, bytes)
}
fn downsample(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (nw,nh)=((w/2).max(1),(h/2).max(1)); let mut out=vec![0; (nw*nh*4) as usize];
    for y in 0..nh { for x in 0..nw { for c in 0..4 {
        let mut sum=0u32;
        for dy in 0..2 {for dx in 0..2 {sum+=pixels[(((y*2+dy).min(h-1)*w+(x*2+dx).min(w-1))*4+c) as usize] as u32;}}
        out[((y*nw+x)*4+c) as usize]=((sum+2)/4) as u8;
    }}}
    out
}
pub fn bounded(size: (u32,u32), limit: u32) -> (u32,u32) {
    let (w,h)=(size.0.max(1),size.1.max(1)); let scale=(limit.max(1) as f64/w.max(h) as f64).min(1.0);
    ((w as f64*scale).round().max(1.0) as u32,(h as f64*scale).round().max(1.0) as u32)
}
fn decode_stars(bytes: &[u8]) -> anyhow::Result<Vec<Star>> {
    anyhow::ensure!(!bytes.is_empty() && bytes.len()%8==0, "Space catalogue is truncated");
    let mut stars=Vec::with_capacity(bytes.len()/8);
    for b in bytes.chunks_exact(8) {
        let ra=u16::from_le_bytes([b[0],b[1]]) as f32/65535.0*std::f32::consts::TAU;
        let dec=i16::from_le_bytes([b[2],b[3]]) as f32/32767.0*std::f32::consts::FRAC_PI_2;
        let bv=i16::from_le_bytes([b[6],b[7]]);
        stars.push(Star { direction: [dec.cos()*ra.cos(),dec.cos()*ra.sin(),dec.sin()], magnitude: i16::from_le_bytes([b[4],b[5]]) as f32/100.0,
            bv: if bv==i16::MIN {0.35} else {bv as f32/1000.0} });
    } Ok(stars)
}
fn finite(v:f32,fallback:f32)->f32 {if v.is_finite(){v}else{fallback}}
fn dot(a:[f32;3],b:[f32;3])->f32 {a.into_iter().zip(b).map(|(a,b)|a*b).sum()}
fn smooth(a:f32,b:f32,x:f32)->f32 {let t=((x-a)/(b-a)).clamp(0.0,1.0);t*t*(3.0-2.0*t)}
/// Candidate anchors, in output pixels; caller measures/clips real native text.
/// Returning candidates rather than a label bitmap keeps Home typography native.
pub fn label_candidates(size:(u32,u32), params:SpaceParams) -> Vec<(&'static str,[f32;2],f32)> {
    let p=params.clean();if !p.lines {return Vec::new();}
    let (w,h)=(size.0.max(1) as f32,size.1.max(1) as f32);let aspect=w/h;let ph=p.phase-0.5;
    let offset=[(ph*0.10+(1.0-p.blend)*-0.12)*aspect.min(1.0)/aspect,ph*0.04+(1.0-p.blend)*0.045];
    let (ra,dec)=(82.5_f32.to_radians(),4.0_f32.to_radians());
    let forward=[dec.cos()*ra.cos(),dec.cos()*ra.sin(),dec.sin()];
    let right=[ra.sin(),-ra.cos(),0.0];let up=[-dec.sin()*ra.cos(),-dec.sin()*ra.sin(),dec.cos()];
    let mut out=Vec::with_capacity(8);
    for &(name,anchor) in catalog::LABELS {
        let z=dot(anchor,forward);if z<0.1 {continue;}
        let x=(0.5+2.0*dot(anchor,right)/z/aspect-offset[0])*w+7.0;
        let y=(0.5-2.0*dot(anchor,up)/z+offset[1])*h+12.0;
        if x<w*0.03 || x>w*0.97 || y<h*0.055 || y>h*0.91 {continue;}
        let vis=visibility([x,y],size,p); if vis<0.01 {continue;}
        let t=y/h;let edge=0.08*(1.0-(t/0.20).clamp(0.0,1.0))+0.75*((t-0.70)/0.30).clamp(0.0,1.0);
        out.push((name,[x,y],0.62*(0.08+0.92*smooth(0.15,0.90,p.blend))*vis*(1.0-edge)));
    }out
}
pub fn visibility(pixel:[f32;2],size:(u32,u32),p:SpaceParams)->f32 {
    if p.blend>0.999 {return 1.0;}
    let (w,h)=(size.0.max(1) as f32,size.1.max(1) as f32);
    let v=[(pixel[0]-0.5*w)/h,(0.5*h-pixel[1])/h,-2.0];let length=dot(v,v).sqrt();let rd=v.map(|x|x/length);
    let c=p.planet_center();let b=dot(rd,c);let disc=b*b-dot(c,c)+PLANET_RADIUS*PLANET_RADIUS;
    if disc>=0.0 && b-disc.sqrt()>0.0 {return 0.0;}
    if b<=0.0 {1.0}else{smooth(0.0,0.035,(dot(c,c)-b*b).max(0.0).sqrt()-PLANET_RADIUS)}
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn native_shader_validates() {
        let source=include_str!("space.wgsl");
        let m=naga::front::wgsl::parse_str(source).unwrap_or_else(|e|panic!("{}",e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m)
            .unwrap_or_else(|e|panic!("{}",e.emit_to_string(source)));
    }
    #[test] fn shader_abi_is_packed_and_bounded() {assert_eq!(std::mem::size_of::<Uniforms>(),64);assert_eq!(std::mem::size_of::<Star>(),20);assert_eq!(std::mem::size_of::<Segment>(),24);}
    #[test] fn catalogue_and_figures_are_complete() {
        let stars=decode_stars(include_bytes!("space-data/stars.bin")).unwrap();assert_eq!(stars.len(),9827);assert_eq!(catalog::LINES.len(),674);assert_eq!(catalog::LABELS.len(),88);
        for star in &stars {assert!((dot(star.direction,star.direction)-1.0).abs()<0.00001);}
        assert!(catalog::LINES.iter().flatten().all(|i|(*i as usize)<stars.len()));assert!(decode_stars(&[1,2,3]).is_err());
    }
    #[test] fn output_caps_preserve_aspect() {assert_eq!(bounded((2400,1200),1200),(1200,600));assert_eq!(bounded((300,1200),720),(180,720));assert_eq!(bounded((0,0),720),(1,1));}
    #[test] fn invalid_params_are_sanitized() {let p=SpaceParams{phase:f32::NAN,blend:9.0,time:f32::NEG_INFINITY,..Default::default()}.clean();assert!(p.phase.is_finite());assert_eq!(p.blend,1.0);assert_eq!(p.time,0.0);}
    #[test] fn mipmaps_keep_channel_order_and_solid_values() {assert_eq!(downsample(&[12,34,56,255].repeat(8),4,2),[12,34,56,255].repeat(2));}
    #[test] fn sphere_occludes_ground_not_distant_view() {let p=SpaceParams::default();assert_eq!(visibility([1000.,650.],(1100,700),p),0.0);assert_eq!(visibility([1000.,650.],(1100,700),SpaceParams{blend:1.0,..p}),1.0);}
    #[test] fn image_buffers_are_checked_before_gpu_upload() {assert!(SpaceImage{width:2,height:2,rgba:vec![0;15]}.validate().is_err());assert!(SpaceImage{width:1,height:1,rgba:vec![0;4]}.validate().is_ok());}
}
