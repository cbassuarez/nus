//! A ground-view atmosphere with a bounded, reusable volumetric cloud cache.
//!
//! The 3D cloud pass runs at most twice a second during ordinary wind motion.
//! A cheaper pass reprojects that cache, moves independent upper layers, and
//! composites the current sky and Moon at up to 24 Hz. Callers stop requesting
//! frames while the home is hidden or held; identical inputs do no GPU work.
use crate::{Gpu, TextureBinder};
use bytemuck::{Pod, Zeroable};
use std::{sync::Arc, time::Instant};

const VOLUME_MAX: (u32, u32) = (768, 512);
const OUTPUT_MAX: (u32, u32) = (1100, 700);
const OVERSCAN: f32 = 1.12;
const PRESENT_INTERVAL: f32 = 1.0 / 24.0;
const VOLUME_INTERVAL: f32 = 0.5;

/// A planet or other moving point of light, already East/Up/North.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub direction: [f32; 3],
    pub magnitude: f32,
    /// B−V colour index: negative blue, ~0.6 white-yellow, above 1 orange.
    pub bv: f32,
}

/// The real sky: the Hipparcos stars and constellation figures turned to this
/// place and time, the planets, and the Sun and Moon as they meet. Absent, the
/// sky keeps its decorative night.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Celestial {
    /// J2000 equatorial → East/Up/North, row-major.
    pub rotation: [[f32; 3]; 3],
    pub planets: [Option<Mark>; MAX_PLANETS],
    /// Angular radii of the Sun and Moon as seen from here, radians.
    pub sun_radius: f32,
    pub moon_radius: f32,
    /// Fraction of the Sun's disc still showing; 1 when no eclipse is under way.
    pub sun_visible: f32,
    /// How much of the corona to show, 0..1 (total eclipse only).
    pub corona: f32,
    pub ecliptic_north: [f32; 3],
    /// The Earth's shadow axis at the Moon's distance and its radii; a zero
    /// umbra radius means the Moon is clear of it.
    pub shadow_direction: [f32; 3],
    pub umbra_radius: f32,
    pub penumbra_radius: f32,
    /// How much moonlight spoils the dark, 0..1.
    pub moon_glow: f32,
    /// Constellation figures, drawn a stroke at a time: 0 hidden, 1 complete.
    pub lines: f32,
    /// How far modelled clouds part for the show (an eclipse), 0..1. The caller applies it
    /// after the weather has eased, so it takes effect at once; the renderer only repaints for it.
    pub clear_sky: f32,
}

/// Seven naked-eye-and-telescopic planets fit in the catalogue's spare rows.
pub const MAX_PLANETS: usize = 7;

/// Directions are normalized East, Up, North vectors. Wind is the direction
/// the air moves *toward*, in east/north metres per second (not a wind bearing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyParams {
    /// The real star sky, when this has a place and a time.
    pub celestial: Option<Celestial>,
    pub sun_direction: [f32; 3],
    pub moon_direction: [f32; 3],
    pub moon_illumination: f32,
    pub moon_waxing: bool,
    /// Cover fractions are 0..1; shapes are modeled, not reconstructed weather.
    pub low_cover: f32,
    pub mid_cover: f32,
    pub high_cover: f32,
    /// 0 gives sculpted cumulus; 1 gives a layered stratus ceiling.
    pub stratus: f32,
    pub precipitation_mm_h: f32,
    pub cloud_base_km: f32,
    pub haze: f32,
    pub wind_low: [f32; 2],
    pub wind_mid: [f32; 2],
    pub wind_high: [f32; 2],
    /// Local animation seconds. Hold this while paused; never pass Unix time.
    pub time: f32,
    pub seed: u32,
    /// Camera bearing clockwise from north, elevation, and vertical FOV, radians.
    pub view_azimuth: f32,
    pub view_elevation: f32,
    pub fov_y: f32,
    /// Normalized top-left xywh; zero size disables prompt protection.
    pub reading_rect: [f32; 4],
    /// Second protected core, in the same top-left normalized coordinates.
    pub reading_footer: [f32; 4],
    /// Inclusive WCAG relative-luminance bounds; [0, 1] leaves colors unchanged.
    pub reading_luminance: [f32; 2],
    /// Fade distance outside each core, as a fraction of viewport height.
    pub reading_feather: f32,
    /// Legacy artistic veil, used only with unconstrained luminance bounds.
    pub reading_strength: f32,
}
impl Default for SkyParams {
    fn default() -> Self {
        Self {
            celestial: None,
            sun_direction: unit([0.6755, 0.475, 0.64], [0.0, 1.0, 0.0]),
            moon_direction: [0.0, -1.0, 0.0],
            moon_illumination: 0.0,
            moon_waxing: true,
            low_cover: 0.43,
            mid_cover: 0.0,
            high_cover: 0.09,
            stratus: 0.0,
            precipitation_mm_h: 0.0,
            cloud_base_km: 1.7,
            haze: 0.0,
            wind_low: [-15.0, -5.0],
            wind_mid: [-20.0, -4.0],
            wind_high: [-23.0, 0.0],
            time: 0.0,
            seed: 42,
            view_azimuth: 0.0,
            view_elevation: 33.0_f32.to_radians(),
            fov_y: 56.0_f32.to_radians(),
            reading_rect: [0.0; 4],
            reading_footer: [0.0; 4],
            reading_luminance: [0.0, 1.0],
            reading_feather: 0.04,
            reading_strength: 0.0,
        }
    }
}
impl SkyParams {
    fn clean(mut self) -> Self {
        let d = Self::default();
        self.sun_direction = unit(self.sun_direction, d.sun_direction);
        self.moon_direction = unit(self.moon_direction, d.moon_direction);
        for v in [
            &mut self.low_cover,
            &mut self.mid_cover,
            &mut self.high_cover,
            &mut self.stratus,
            &mut self.haze,
            &mut self.moon_illumination,
        ] {
            *v = finite(*v, 0.0).clamp(0.0, 1.0);
        }
        self.cloud_base_km = finite(self.cloud_base_km, 1.7).clamp(0.15, 8.0);
        self.precipitation_mm_h = finite(self.precipitation_mm_h, 0.0).clamp(0.0, 100.0);
        self.time = finite(self.time, 0.0).max(0.0);
        self.view_azimuth = finite(self.view_azimuth, 0.0);
        self.view_elevation = finite(self.view_elevation, d.view_elevation).clamp(0.12, 1.35);
        self.fov_y = finite(self.fov_y, d.fov_y).clamp(0.2, 1.6);
        for wind in [&mut self.wind_low, &mut self.wind_mid, &mut self.wind_high] {
            for v in wind {
                *v = finite(*v, 0.0).clamp(-120.0, 120.0);
            }
        }
        for rect in [&mut self.reading_rect, &mut self.reading_footer] {
            for v in rect {
                *v = finite(*v, 0.0).clamp(0.0, 1.0);
            }
        }
        self.reading_luminance[0] = finite(self.reading_luminance[0], 0.0).clamp(0.0, 1.0);
        self.reading_luminance[1] =
            finite(self.reading_luminance[1], 1.0).clamp(self.reading_luminance[0], 1.0);
        self.reading_feather = finite(self.reading_feather, 0.04).clamp(0.0, 1.0);
        self.reading_strength = finite(self.reading_strength, 0.0).clamp(0.0, 0.35);
        if let Some(c) = &mut self.celestial {
            c.rotation = c.rotation.map(|row| row.map(|v| finite(v, 0.0)));
            for mark in c.planets.iter_mut().flatten() {
                mark.direction = unit(mark.direction, [0.0, -1.0, 0.0]);
                mark.magnitude = finite(mark.magnitude, 99.0);
                mark.bv = finite(mark.bv, 0.6);
            }
            c.sun_radius = finite(c.sun_radius, 0.0).clamp(0.0, 0.05);
            c.moon_radius = finite(c.moon_radius, 0.0).clamp(0.0, 0.05);
            c.sun_visible = finite(c.sun_visible, 1.0).clamp(0.0, 1.0);
            c.corona = finite(c.corona, 0.0).clamp(0.0, 1.0);
            c.ecliptic_north = unit(c.ecliptic_north, [0.0, 1.0, 0.0]);
            c.shadow_direction = unit(c.shadow_direction, [0.0, -1.0, 0.0]);
            c.umbra_radius = finite(c.umbra_radius, 0.0).clamp(0.0, 0.2);
            c.penumbra_radius = finite(c.penumbra_radius, 0.0).clamp(0.0, 0.3);
            c.moon_glow = finite(c.moon_glow, 0.0).clamp(0.0, 1.0);
            c.lines = finite(c.lines, 0.0).clamp(0.0, 1.0);
            c.clear_sky = finite(c.clear_sky, 0.0).clamp(0.0, 1.0);
        }
        self
    }
}
/// Counts distinguish expensive cache updates, cheap presentation and no work.
#[derive(Debug, Default, Clone, Copy)]
pub struct SkyStats {
    pub volume_draws: u64,
    pub present_draws: u64,
    pub reuses: u64,
    pub volume_size: (u32, u32),
    pub output_size: (u32, u32),
    pub texture_bytes: u64,
    /// Queue submission and command encoding; this is not a GPU measurement.
    pub cpu_submit_ms: f64,
    pub init_pipeline_cpu_ms: f64,
    pub init_noise_cpu_ms: f64,
    /// Available only after explicit `read_gpu_timings` on a timestamp device.
    pub gpu_volume_ms: Option<f64>,
    pub gpu_present_ms: Option<f64>,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    resolution: [f32; 4],
    sun: [f32; 4],
    moon: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    weather: [f32; 4],
    air: [f32; 4],
    wind_low: [f32; 4],
    wind_mid: [f32; 4],
    wind_high: [f32; 4],
    cache: [f32; 4],
    cache_wind: [f32; 4],
    reading: [f32; 4],
    reading_footer: [f32; 4],
    protection: [f32; 4],
    rot0: [f32; 4],
    rot1: [f32; 4],
    rot2: [f32; 4],
    body: [f32; 4],
    shadow: [f32; 4],
    lunar: [f32; 4],
    ecl_north: [f32; 4],
}
/// One constellation stroke: the two stars, and where it falls in its figure.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Stroke {
    a: [f32; 3],
    b: [f32; 3],
    /// Start of the stroke, and its share, within its figure's 0..1 reveal.
    order: [f32; 2],
}
const STAR_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32];
const STROKE_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
struct Timing {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    pending: bool,
    volume: bool,
}
struct Targets {
    _volume: wgpu::Texture,
    volume_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    present_bind: wgpu::BindGroup,
    scene_bind: Arc<wgpu::BindGroup>,
    size: (u32, u32),
}
pub struct SkyRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    scene_binder: Option<TextureBinder>,
    fallback_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    noise_sampler: wgpu::Sampler,
    noise: wgpu::Texture,
    noise_seed: u32,
    uniform: wgpu::Buffer,
    cloud_bgl: wgpu::BindGroupLayout,
    present_bgl: wgpu::BindGroupLayout,
    cloud_bind: wgpu::BindGroup,
    cloud_pipeline: wgpu::RenderPipeline,
    present_pipeline: wgpu::RenderPipeline,
    star_pipeline: wgpu::RenderPipeline,
    stroke_pipeline: wgpu::RenderPipeline,
    star_bind: wgpu::BindGroup,
    /// The packed catalogue, then the planets' rows (rewritten as they move).
    stars: wgpu::Buffer,
    strokes: wgpu::Buffer,
    star_count: u32,
    stroke_count: u32,
    targets: Option<Targets>,
    previous: Option<SkyParams>,
    presented: Option<SkyParams>,
    cache_params: Option<SkyParams>,
    last_time: Option<f32>,
    displacement: [[f32; 2]; 3],
    cache_displacement: [f32; 2],
    cache_time: f32,
    last_present: f32,
    last_submit: Instant,
    last_volume: Instant,
    timing: Option<Timing>,
    stats: SkyStats,
}
impl SkyRenderer {
    pub fn new(gpu: &Gpu) -> Self {
        Self::build(&gpu.device, &gpu.queue, Some(gpu.texture_binder()))
    }
    /// Standalone validation/capture path. Its returned bind group is not a
    /// Scene binding; native windows should use `new(&Gpu)`.
    pub fn new_offscreen(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self::build(device, queue, None)
    }
    fn build(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene_binder: Option<TextureBinder>,
    ) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let noise_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky volume repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky cache clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let cloud_entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            texture_entry(1, wgpu::TextureViewDimension::D3),
            sampler_entry(2),
        ];
        let cloud_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky cloud layout"),
            entries: &cloud_entries,
        });
        let present_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky presentation layout"),
            entries: &[
                cloud_entries[0],
                cloud_entries[1],
                cloud_entries[2],
                texture_entry(3, wgpu::TextureViewDimension::D2),
                sampler_entry(4),
            ],
        });
        let fallback_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky standalone texture layout"),
            entries: &[
                texture_entry(0, wgpu::TextureViewDimension::D2),
                sampler_entry(1),
            ],
        });
        let pipeline_started = Instant::now();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ground sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let pipeline = |label,
                        vertex: &'static str,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        entry,
                        bgl: &wgpu::BindGroupLayout,
                        format,
                        blend| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(bgl)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let cloud_pipeline = pipeline(
            "sky volume",
            "vs_main",
            &[],
            "cloud_main",
            &cloud_bgl,
            wgpu::TextureFormat::Rgba16Float,
            None,
        );
        // The presentation lays the sky over whatever the star layer put in the target:
        // colour + stars × (1 − alpha), alpha kept at the opaque clear.
        let over_stars = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let present_pipeline = pipeline(
            "sky presentation",
            "vs_main",
            &[],
            "present_main",
            &present_bgl,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(over_stars),
        );
        let star_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky star layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ..cloud_entries[0]
            }],
        });
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            // The opaque base stays opaque; star fragments carry zero alpha.
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let star_pipeline = pipeline(
            "sky stars",
            "star_vs",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<crate::space::Star>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &STAR_ATTRIBUTES,
            })],
            "star_fs",
            &star_bgl,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(additive),
        );
        let stroke_pipeline = pipeline(
            "sky constellation strokes",
            "line_vs",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Stroke>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &STROKE_ATTRIBUTES,
            })],
            "line_fs",
            &star_bgl,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(additive),
        );
        let star_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky star inputs"),
            layout: &star_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let (stars, star_count, strokes, stroke_count) = star_buffers(device);
        let init_pipeline_cpu_ms = pipeline_started.elapsed().as_secs_f64() * 1000.0;
        let noise_started = Instant::now();
        let noise = make_noise(device, queue, 42);
        let init_noise_cpu_ms = noise_started.elapsed().as_secs_f64() * 1000.0;
        let cloud_bind = cloud_binding(device, &cloud_bgl, &uniform, &noise, &noise_sampler);
        let timing = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| Timing {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("sky timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 4,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sky timing resolve"),
                    size: 32,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sky timing readback"),
                    size: 32,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                pending: false,
                volume: false,
            });
        Self {
            device: device.clone(),
            queue: queue.clone(),
            scene_binder,
            fallback_bgl,
            sampler,
            noise_sampler,
            noise,
            noise_seed: 42,
            uniform,
            cloud_bgl,
            present_bgl,
            cloud_bind,
            cloud_pipeline,
            present_pipeline,
            star_pipeline,
            stroke_pipeline,
            star_bind,
            stars,
            strokes,
            star_count,
            stroke_count,
            targets: None,
            previous: None,
            presented: None,
            cache_params: None,
            last_time: None,
            displacement: [[0.0; 2]; 3],
            cache_displacement: [0.0; 2],
            cache_time: 0.0,
            last_present: f32::NEG_INFINITY,
            last_submit: Instant::now(),
            last_volume: Instant::now(),
            timing,
            stats: SkyStats {
                init_pipeline_cpu_ms,
                init_noise_cpu_ms,
                ..Default::default()
            },
        }
    }
    pub fn stats(&self) -> SkyStats {
        self.stats
    }
    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.output)
    }
    pub fn render(
        &mut self,
        _gpu: &Gpu,
        size: (u32, u32),
        params: SkyParams,
    ) -> Arc<wgpu::BindGroup> {
        self.render_offscreen(size, params)
    }
    pub fn render_offscreen(
        &mut self,
        size: (u32, u32),
        params: SkyParams,
    ) -> Arc<wgpu::BindGroup> {
        let start = Instant::now();
        let p = params.clean();
        let size = (size.0.max(1), size.1.max(1));
        // Integrate velocity, so a forecast wind change never multiplies the
        // entire lifetime by a new wind and teleports the cloud field.
        if let Some(last) = self.last_time {
            let dt = if p.time >= last {
                (p.time - last).min(5.0)
            } else {
                0.0
            };
            let old = self.previous.unwrap_or(p);
            for (i, (wind, prior)) in [p.wind_low, p.wind_mid, p.wind_high]
                .into_iter()
                .zip([old.wind_low, old.wind_mid, old.wind_high])
                .enumerate()
            {
                for axis in 0..2 {
                    self.displacement[i][axis] += (wind[axis] + prior[axis]) * 0.5 * dt * 0.001;
                }
            }
        }
        self.last_time = Some(p.time);
        let resized = self.targets.as_ref().is_none_or(|t| t.size != size);
        let seed_changed = self.noise_seed != p.seed;
        if seed_changed {
            self.noise = make_noise(&self.device, &self.queue, p.seed);
            self.noise_seed = p.seed;
            self.cloud_bind = cloud_binding(
                &self.device,
                &self.cloud_bgl,
                &self.uniform,
                &self.noise,
                &self.noise_sampler,
            );
        }
        if resized || seed_changed {
            self.make_targets(size);
        }
        let structural = self.presented.is_none_or(|old| present_changed(old, p));
        let low_present = p.low_cover > 0.001 || p.precipitation_mm_h > 0.01;
        let moving = self.previous.is_some_and(|old| p.time != old.time)
            && [
                (low_present, p.wind_low),
                (p.mid_cover > 0.001, p.wind_mid),
                (p.high_cover > 0.001, p.wind_high),
            ]
            .iter()
            .any(|(visible, w)| *visible && w[0].abs() + w[1].abs() > 0.01);
        let due =
            p.time - self.last_present >= PRESENT_INTERVAL - 0.00025 || p.time < self.last_present;
        let camera_changed = self.cache_params.is_none_or(|old| camera_diff(old, p));
        let weather_changed = self.cache_params.is_none_or(|old| volume_changed(old, p));
        let displacement = (self.displacement[0][0] - self.cache_displacement[0])
            .hypot(self.displacement[0][1] - self.cache_displacement[1]);
        let volume_due = p.time - self.cache_time >= VOLUME_INTERVAL
            || p.time < self.cache_time
            || self.last_volume.elapsed().as_secs_f32() >= VOLUME_INTERVAL;
        let volume = resized
            || seed_changed
            || camera_changed
            || (weather_changed
                && (volume_due || self.previous.is_some_and(|old| old.time == p.time)))
            || (low_present && displacement > 0.00025 && volume_due);
        // A due, pending volume refresh must not be stranded by a held frame.
        if !volume && !resized && !seed_changed && !structural && (!moving || !due) {
            self.previous = Some(p);
            self.stats.reuses += 1;
            return self.targets.as_ref().unwrap().scene_bind.clone();
        }
        if volume {
            self.cache_time = p.time;
            self.cache_displacement = self.displacement[0];
            self.cache_params = Some(p);
            self.last_volume = Instant::now();
        }
        let uniforms = self.uniforms(size, p);
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniforms));
        if let Some(c) = &p.celestial {
            // Planets ride in the catalogue's spare rows, turned back into the J2000 frame the
            // shader will turn forward again (the rotation is orthonormal: its inverse is its transpose).
            let rows: [crate::space::Star; MAX_PLANETS] =
                std::array::from_fn(|i| match c.planets[i] {
                    Some(m) => crate::space::Star {
                        direction: std::array::from_fn(|j| {
                            c.rotation[0][j] * m.direction[0]
                                + c.rotation[1][j] * m.direction[1]
                                + c.rotation[2][j] * m.direction[2]
                        }),
                        magnitude: m.magnitude,
                        bv: m.bv,
                    },
                    None => crate::space::Star {
                        direction: [0.0, 0.0, 1.0],
                        magnitude: 99.0,
                        bv: 0.6,
                    },
                });
            let offset = u64::from(self.star_count - MAX_PLANETS as u32)
                * std::mem::size_of::<crate::space::Star>() as u64;
            self.queue
                .write_buffer(&self.stars, offset, bytemuck::cast_slice(&rows));
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ground sky cache"),
            });
        let targets = self.targets.as_ref().unwrap();
        if volume {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky volume cache"),
                color_attachments: &[Some(attachment(&targets.volume_view))],
                timestamp_writes: self
                    .timing
                    .as_ref()
                    .map(|t| wgpu::RenderPassTimestampWrites {
                        query_set: &t.queries,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                ..Default::default()
            });
            pass.set_pipeline(&self.cloud_pipeline);
            pass.set_bind_group(0, &self.cloud_bind, &[]);
            pass.draw(0..3, 0..1);
            self.stats.volume_draws += 1;
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky cached presentation"),
                color_attachments: &[Some(attachment_opaque(&targets.output_view))],
                timestamp_writes: self
                    .timing
                    .as_ref()
                    .map(|t| wgpu::RenderPassTimestampWrites {
                        query_set: &t.queries,
                        beginning_of_pass_write_index: Some(2),
                        end_of_pass_write_index: Some(3),
                    }),
                ..Default::default()
            });
            if let Some(c) = &p.celestial {
                // Stars and figures first; the sky is composited over them.
                pass.set_bind_group(0, &self.star_bind, &[]);
                pass.set_pipeline(&self.star_pipeline);
                pass.set_vertex_buffer(0, self.stars.slice(..));
                pass.draw(0..6, 0..self.star_count);
                if c.lines > 0.0 {
                    pass.set_pipeline(&self.stroke_pipeline);
                    pass.set_vertex_buffer(0, self.strokes.slice(..));
                    pass.draw(0..6, 0..self.stroke_count);
                }
            }
            pass.set_pipeline(&self.present_pipeline);
            pass.set_bind_group(0, &targets.present_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(t) = &mut self.timing {
            // Queries 0..2 are only copied when initialized by a volume pass.
            if volume {
                encoder.resolve_query_set(&t.queries, 0..4, &t.resolve, 0);
                encoder.copy_buffer_to_buffer(&t.resolve, 0, &t.readback, 0, 32);
            } else {
                encoder.resolve_query_set(&t.queries, 2..4, &t.resolve, 0);
                encoder.copy_buffer_to_buffer(&t.resolve, 0, &t.readback, 16, 16);
            }
            t.pending = true;
            t.volume = volume;
        }
        self.queue.submit([encoder.finish()]);
        self.last_present = p.time;
        self.previous = Some(p);
        self.presented = Some(p);
        self.last_submit = Instant::now();
        self.stats.present_draws += 1;
        self.stats.cpu_submit_ms = start.elapsed().as_secs_f64() * 1000.0;
        targets.scene_bind.clone()
    }
    fn make_targets(&mut self, size: (u32, u32)) {
        let output_size = bounded(size, OUTPUT_MAX);
        let volume_size = bounded(size, VOLUME_MAX);
        let texture = |label, (w, h), format| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let volume = texture(
            "sky volume cache",
            volume_size,
            wgpu::TextureFormat::Rgba16Float,
        );
        let output = texture(
            "sky presentation cache",
            output_size,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let volume_view = volume.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let noise_view = self.noise.create_view(&Default::default());
        let present_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky cached inputs"),
            layout: &self.present_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&noise_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.noise_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&volume_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let scene_bind = if let Some(binder) = &self.scene_binder {
            binder.bind(&output)
        } else {
            Arc::new(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky capture output"),
                layout: &self.fallback_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&output_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }))
        };
        self.targets = Some(Targets {
            _volume: volume,
            volume_view,
            output,
            output_view,
            present_bind,
            scene_bind,
            size,
        });
        self.stats.volume_size = volume_size;
        self.stats.output_size = output_size;
        self.stats.texture_bytes = 64 * 64 * 64 * 4
            + u64::from(volume_size.0) * u64::from(volume_size.1) * 8
            + u64::from(output_size.0) * u64::from(output_size.1) * 4;
    }
    fn uniforms(&self, size: (u32, u32), p: SkyParams) -> Uniforms {
        let (sa, ca) = p.view_azimuth.sin_cos();
        let (se, ce) = p.view_elevation.sin_cos();
        let wind =
            |v: [f32; 2], i: usize| [v[0], v[1], self.displacement[i][0], self.displacement[i][1]];
        Uniforms {
            resolution: [
                self.stats.output_size.0 as f32,
                self.stats.output_size.1 as f32,
                size.0 as f32 / size.1 as f32,
                OVERSCAN,
            ],
            sun: [
                p.sun_direction[0],
                p.sun_direction[1],
                p.sun_direction[2],
                p.moon_illumination,
            ],
            moon: [
                p.moon_direction[0],
                p.moon_direction[1],
                p.moon_direction[2],
                if p.moon_waxing { 1.0 } else { -1.0 },
            ],
            right: [ca, 0.0, -sa, (p.fov_y * 0.5).tan()],
            up: [-sa * se, ce, -ca * se, 0.0],
            forward: [sa * ce, se, ca * ce, 0.0],
            weather: [p.low_cover, p.mid_cover, p.high_cover, p.stratus],
            air: [p.precipitation_mm_h, p.haze, p.cloud_base_km, p.time],
            wind_low: wind(p.wind_low, 0),
            wind_mid: wind(p.wind_mid, 1),
            wind_high: wind(p.wind_high, 2),
            cache: [
                self.cache_time,
                p.cloud_base_km + 0.75,
                p.seed as f32 * 0.001,
                0.0,
            ],
            cache_wind: [
                self.cache_displacement[0],
                self.cache_displacement[1],
                0.0,
                0.0,
            ],
            reading: p.reading_rect,
            reading_footer: p.reading_footer,
            protection: [
                p.reading_strength,
                p.reading_luminance[0],
                p.reading_luminance[1],
                p.reading_feather,
            ],
            rot0: p.celestial.map_or([0.0; 4], |c| {
                [c.rotation[0][0], c.rotation[0][1], c.rotation[0][2], 1.0]
            }),
            rot1: p.celestial.map_or([0.0; 4], |c| {
                [c.rotation[1][0], c.rotation[1][1], c.rotation[1][2], 0.0]
            }),
            rot2: p.celestial.map_or([0.0; 4], |c| {
                [c.rotation[2][0], c.rotation[2][1], c.rotation[2][2], 0.0]
            }),
            body: p.celestial.map_or([0.0, 0.0, 1.0, 0.0], |c| {
                [c.sun_radius, c.moon_radius, c.sun_visible, c.corona]
            }),
            shadow: p.celestial.map_or([0.0; 4], |c| {
                [
                    c.shadow_direction[0],
                    c.shadow_direction[1],
                    c.shadow_direction[2],
                    c.umbra_radius,
                ]
            }),
            lunar: p
                .celestial
                .map_or([0.0; 4], |c| [c.penumbra_radius, c.moon_glow, c.lines, 0.0]),
            ecl_north: p.celestial.map_or([0.0, 1.0, 0.0, 0.0], |c| {
                [
                    c.ecliptic_north[0],
                    c.ecliptic_north[1],
                    c.ecliptic_north[2],
                    0.0,
                ]
            }),
        }
    }
    /// Blocking diagnostic only. Production rendering never maps a timing
    /// buffer or waits for the GPU. `None` means timestamps are unsupported.
    pub fn read_gpu_timings(&mut self) -> anyhow::Result<Option<(Option<f64>, Option<f64>)>> {
        let Some(t) = self.timing.as_mut().filter(|t| t.pending) else {
            return Ok(None);
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let slice = t.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let mapped = slice.get_mapped_range()?;
        let values: &[u64] = bytemuck::cast_slice(&mapped);
        let period_ms = self.queue.get_timestamp_period() as f64 / 1_000_000.0;
        // Some Metal adapters coalesce timestamp samples. A zero interval is
        // unresolved, not evidence that a render pass has zero GPU cost.
        let volume =
            (t.volume && values[1] > values[0]).then(|| (values[1] - values[0]) as f64 * period_ms);
        let present = (values[3] > values[2]).then(|| (values[3] - values[2]) as f64 * period_ms);
        drop(mapped);
        t.readback.unmap();
        t.pending = false;
        self.stats.gpu_volume_ms = volume;
        self.stats.gpu_present_ms = present;
        Ok(Some((volume, present)))
    }
}
fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}
fn unit(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !length.is_finite() || length < 0.0001 {
        fallback
    } else {
        v.map(|x| x / length)
    }
}
fn bounded(size: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    let scale = 1.0_f64
        .min(max.0 as f64 / size.0.max(1) as f64)
        .min(max.1 as f64 / size.1.max(1) as f64);
    (
        (size.0 as f64 * scale).round().max(1.0) as u32,
        (size.1 as f64 * scale).round().max(1.0) as u32,
    )
}
/// The catalogue's stars, then the planets' spare rows; and the figures' strokes.
fn star_buffers(device: &wgpu::Device) -> (wgpu::Buffer, u32, wgpu::Buffer, u32) {
    use wgpu::util::DeviceExt;
    let mut rows: Vec<crate::space::Star> = crate::space::catalogue().to_vec();
    rows.extend((0..MAX_PLANETS).map(|_| crate::space::Star {
        direction: [0.0, 0.0, 1.0],
        magnitude: 99.0,
        bv: 0.6,
    }));
    let stars = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sky stars and planets"),
        contents: bytemuck::cast_slice(&rows),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    let catalogue = crate::space::catalogue();
    let mut strokes = Vec::new();
    for figure in crate::space::figures() {
        let n = f32::from(figure.end - figure.first).max(1.0);
        for (k, pair) in crate::space::figure_segments()[figure.first as usize..figure.end as usize]
            .iter()
            .enumerate()
        {
            strokes.push(Stroke {
                a: catalogue[pair[0] as usize].direction,
                b: catalogue[pair[1] as usize].direction,
                order: [k as f32 / n, 1.0 / n],
            });
        }
    }
    let strokes_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sky constellation strokes"),
        contents: bytemuck::cast_slice(&strokes),
        usage: wgpu::BufferUsages::VERTEX,
    });
    (
        stars,
        rows.len() as u32,
        strokes_buffer,
        strokes.len() as u32,
    )
}
/// How far apart two real skies are, for deciding whether a repaint is due.
fn celestial_changed(a: SkyParams, b: SkyParams) -> bool {
    let (x, y) = match (a.celestial, b.celestial) {
        (None, None) => return false,
        (Some(x), Some(y)) => (x, y),
        _ => return true,
    };
    // The sky turns 15° an hour; a few arcseconds is not worth a repaint.
    let turned = x
        .rotation
        .iter()
        .flatten()
        .zip(y.rotation.iter().flatten())
        .any(|(p, q)| (p - q).abs() > 6e-5);
    let planets = x.planets.iter().zip(&y.planets).any(|(p, q)| match (p, q) {
        (Some(p), Some(q)) => {
            p.direction
                .iter()
                .zip(q.direction)
                .any(|(a, b)| (a - b).abs() > 5e-5)
                || (p.magnitude - q.magnitude).abs() > 0.02
        }
        (None, None) => false,
        _ => true,
    });
    turned
        || planets
        || (x.sun_radius - y.sun_radius).abs() > 1e-6
        || (x.moon_radius - y.moon_radius).abs() > 1e-6
        || (x.sun_visible - y.sun_visible).abs() > 0.002
        || (x.corona - y.corona).abs() > 0.01
        || (x.umbra_radius - y.umbra_radius).abs() > 1e-5
        || (x.penumbra_radius - y.penumbra_radius).abs() > 1e-5
        || x.shadow_direction
            .iter()
            .zip(y.shadow_direction)
            .any(|(a, b)| (a - b).abs() > 2e-5)
        || (x.moon_glow - y.moon_glow).abs() > 0.01
        || (x.lines - y.lines).abs() > 0.001
        || (x.clear_sky - y.clear_sky).abs() > 0.01
}
/// The Sun's visible fraction, 1 when the real sky is off.
fn sun_visible(p: SkyParams) -> f32 {
    p.celestial.map_or(1.0, |c| c.sun_visible)
}
fn camera_diff(a: SkyParams, b: SkyParams) -> bool {
    (a.view_azimuth - b.view_azimuth).abs() > 0.0001
        || (a.view_elevation - b.view_elevation).abs() > 0.0001
        || (a.fov_y - b.fov_y).abs() > 0.0001
}
fn volume_changed(a: SkyParams, b: SkyParams) -> bool {
    camera_diff(a, b)
        || a.seed != b.seed
        // Clouds are lit by the Sun: an eclipse darkens them.
        || (sun_visible(a) - sun_visible(b)).abs() > 0.03
        || (a.low_cover - b.low_cover).abs() > 0.015
        || (a.stratus - b.stratus).abs() > 0.02
        || (a.cloud_base_km - b.cloud_base_km).abs() > 0.04
        || (a.precipitation_mm_h - b.precipitation_mm_h).abs() > 0.10
        || (a.haze - b.haze).abs() > 0.02
        || a.sun_direction
            .iter()
            .zip(b.sun_direction)
            .any(|(a, b)| (*a - b).abs() > 0.002)
        || (b.sun_direction[1] < 0.0
            && (a
                .moon_direction
                .iter()
                .zip(b.moon_direction)
                .any(|(a, b)| (*a - b).abs() > 0.002)
                || (a.moon_illumination - b.moon_illumination).abs() > 0.01))
}
fn present_changed(a: SkyParams, b: SkyParams) -> bool {
    a.moon_waxing != b.moon_waxing
        || celestial_changed(a, b)
        || camera_diff(a, b)
        || a.seed != b.seed
        || a.reading_rect != b.reading_rect
        || a.reading_footer != b.reading_footer
        || a.reading_luminance != b.reading_luminance
        || a.reading_feather != b.reading_feather
        || a.reading_strength != b.reading_strength
        || [
            a.low_cover - b.low_cover,
            a.mid_cover - b.mid_cover,
            a.high_cover - b.high_cover,
            a.stratus - b.stratus,
            a.haze - b.haze,
            a.precipitation_mm_h - b.precipitation_mm_h,
        ]
        .iter()
        .any(|d| d.abs() > 0.001)
        || (a.cloud_base_km - b.cloud_base_km).abs() > 0.001
        || (a.moon_illumination - b.moon_illumination).abs() > 0.001
        || a.sun_direction
            .iter()
            .zip(b.sun_direction)
            .any(|(a, b)| (*a - b).abs() > 0.00005)
        || a.moon_direction
            .iter()
            .zip(b.moon_direction)
            .any(|(a, b)| (*a - b).abs() > 0.00005)
}
fn texture_entry(
    binding: u32,
    dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: dimension,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
        },
        count: None,
    }
}
fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
        depth_slice: None,
    }
}
/// The star layer starts from opaque black, so the presentation's blend can keep alpha.
fn attachment_opaque(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
        },
        depth_slice: None,
    }
}
fn cloud_binding(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    noise: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let view = noise.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sky cloud inputs"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
fn make_noise(device: &wgpu::Device, queue: &wgpu::Queue, seed: u32) -> wgpu::Texture {
    // The stock seed is baked from `noise_data(42)` byte-for-byte: no synchronous
    // Worley generation during first home presentation. Custom seeds stay procedural.
    let generated;
    let data: &[u8] = if seed == 42 {
        include_bytes!("sky-noise-42.rgba")
    } else {
        generated = noise_data(seed);
        &generated
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sky seeded 3D cloud noise"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 64,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(64 * 4),
            rows_per_image: Some(64),
        },
        wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 64,
        },
    );
    texture
}
fn noise_data(seed: u32) -> Vec<u8> {
    let hash = |x: i32, y: i32, z: i32| {
        let n = (x as u32)
            .wrapping_mul(374761393)
            .wrapping_add((y as u32).wrapping_mul(668265263))
            .wrapping_add((z as u32).wrapping_mul(2147483647))
            .wrapping_add(seed);
        let n = (n ^ (n >> 13)).wrapping_mul(1274126177);
        (n ^ (n >> 16)) as f32 / 4294967296.0
    };
    let value = |p: [f32; 3], n: i32| {
        let v = p.map(|x| x * n as f32);
        let i = v.map(|x| x.floor() as i32);
        let f = std::array::from_fn::<_, 3, _>(|a| {
            let x = v[a] - i[a] as f32;
            x * x * (3.0 - 2.0 * x)
        });
        let mut s = 0.0;
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    s += hash((i[0] + x) % n, (i[1] + y) % n, (i[2] + z) % n)
                        * if x == 1 { f[0] } else { 1.0 - f[0] }
                        * if y == 1 { f[1] } else { 1.0 - f[1] }
                        * if z == 1 { f[2] } else { 1.0 - f[2] };
                }
            }
        }
        s
    };
    let worley = |p: [f32; 3], n: i32| {
        let v = p.map(|x| x * n as f32);
        let q = v.map(|x| x.floor() as i32);
        let mut d = 3.0_f32;
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    let a = q[0] + x;
                    let b = q[1] + y;
                    let c = q[2] + z;
                    let aa = a.rem_euclid(n);
                    let bb = b.rem_euclid(n);
                    let cc = c.rem_euclid(n);
                    let dx = a as f32 + hash(aa, bb, cc) - v[0];
                    let dy = b as f32 + hash(aa + 13, bb + 71, cc + 5) - v[1];
                    let dz = c as f32 + hash(aa + 37, bb + 19, cc + 113) - v[2];
                    d = d.min(dx * dx + dy * dy + dz * dz);
                }
            }
        }
        (1.0 - d.sqrt() * 0.85).max(0.0)
    };
    let mut data = Vec::with_capacity(64 * 64 * 64 * 4);
    for z in 0..64 {
        for y in 0..64 {
            for x in 0..64 {
                let p = [x as f32 / 64.0, y as f32 / 64.0, z as f32 / 64.0];
                for v in [
                    value(p, 4) * 0.62 + value(p, 8) * 0.26 + value(p, 16) * 0.12,
                    worley(p, 4),
                    worley(p, 8),
                    value(p, 16),
                ] {
                    data.push((v * 255.0).round().clamp(0.0, 255.0) as u8);
                }
            }
        }
    }
    data
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stock_noise_preserves_procedural_reference() {
        assert_eq!(
            include_bytes!("sky-noise-42.rgba").as_slice(),
            noise_data(42)
        );
    }

    #[test]
    fn shader_validates() {
        let source = include_str!("sky.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    }
    #[test]
    fn bounds_preserve_aspect_and_memory() {
        assert_eq!(bounded((2200, 1400), OUTPUT_MAX), (1100, 700));
        assert_eq!(bounded((360, 760), VOLUME_MAX), (243, 512));
        assert_eq!(bounded((0, 0), OUTPUT_MAX), (1, 1));
        let (vw, vh) = bounded((2200, 1400), VOLUME_MAX);
        let (ow, oh) = bounded((2200, 1400), OUTPUT_MAX);
        assert!(64 * 64 * 64 * 4 + vw * vh * 8 + ow * oh * 4 < 8 * 1024 * 1024);
    }
    #[test]
    fn direction_retains_azimuth_and_invalid_values_are_bounded() {
        let p = SkyParams {
            sun_direction: [1.0, 0.0, -1.0],
            haze: f32::NAN,
            cloud_base_km: -1.0,
            ..Default::default()
        }
        .clean();
        assert!(p.sun_direction[0] > 0.7 && p.sun_direction[2] < -0.7);
        assert_eq!(p.haze, 0.0);
        assert_eq!(p.cloud_base_km, 0.15);
    }
    #[test]
    fn small_ephemeris_and_reading_changes_do_not_rebuild_volume() {
        let a = SkyParams::default();
        let mut b = a;
        b.sun_direction[0] += 0.0001;
        b.reading_rect = [0.1, 0.2, 0.5, 0.2];
        b.reading_strength = 0.14;
        assert!(!volume_changed(a, b));
        assert!(present_changed(a, b));
    }
}
