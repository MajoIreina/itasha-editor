use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};

pub(super) const SAMPLE_COUNT: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Off,
    Fxaa,
    Taa,
    Taa2x,
    Ssaa,
}
impl Mode {
    pub fn parse(value: &str) -> Self {
        match value {
            "off" => Self::Off,
            "taa" => Self::Taa,
            "taa2x" => Self::Taa2x,
            "ssaa" => Self::Ssaa,
            _ => Self::Fxaa,
        }
    }
    pub fn temporal(self) -> bool {
        matches!(self, Self::Taa | Self::Taa2x)
    }
    pub fn supersampled(self) -> bool {
        matches!(self, Self::Taa2x | Self::Ssaa)
    }
}

/// Bound all intermediate attachments to 256 MiB, independently of model textures.
/// TAA needs scene + depth + two RGBA16F / R32F history pairs (32 bytes/pixel).
pub(super) fn render_extent(
    output: [u32; 2],
    scale: u16,
    mode: Mode,
    limit: u32,
) -> ([u32; 2], bool) {
    let factor = if mode.supersampled() { 2 } else { 1 };
    let requested =
        output.map(|v| ((u64::from(v) * u64::from(scale) / 100).max(1) * factor) as f64);
    let max_pixels = (256_u64 * 1024 * 1024 / if mode.temporal() { 32 } else { 8 }) as f64;
    let fit = (f64::from(limit.max(1)) / requested[0])
        .min(f64::from(limit.max(1)) / requested[1])
        .min((max_pixels / (requested[0] * requested[1])).sqrt())
        .min(1.);
    (
        requested.map(|v| (v * fit).floor().max(1.) as u32),
        fit < 1.,
    )
}

fn halton(mut index: u32, base: u32) -> f32 {
    let mut result = 0.;
    let mut fraction = 1.;
    while index > 0 {
        fraction /= base as f32;
        result += fraction * (index % base) as f32;
        index /= base;
    }
    result
}
pub(super) fn jitter(index: u32) -> Vec2 {
    // Center the finite sequence, not the infinite Halton sequence, so the
    // eight-frame settled image has no persistent subpixel displacement.
    let mean = (1..=SAMPLE_COUNT)
        .map(|i| Vec2::new(halton(i, 2), halton(i, 3)))
        .sum::<Vec2>()
        / SAMPLE_COUNT as f32;
    let i = index % SAMPLE_COUNT + 1;
    Vec2::new(halton(i, 2), halton(i, 3)) - mean
}
pub(super) fn jittered(vp: Mat4, offset: Vec2, size: [u32; 2]) -> Mat4 {
    Mat4::from_translation(Vec3::new(
        2. * offset.x / size[0] as f32,
        -2. * offset.y / size[1] as f32,
        0.,
    )) * vp
}

#[derive(Default)]
struct HistoryState {
    previous: Option<Mat4>,
    samples: u32,
    frames: u32,
    phase: u32,
}
impl HistoryState {
    fn invalidate(&mut self) {
        *self = Self::default();
    }
    fn prepare(&mut self, vp: Mat4) -> (bool, f32) {
        let difference = self
            .previous
            .map(|old| {
                old.to_cols_array()
                    .into_iter()
                    .zip(vp.to_cols_array())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0_f32, f32::max)
            })
            .unwrap_or(0.);
        let moving = difference > 1e-6;
        if moving {
            self.samples = 0;
        }
        if difference > 0.5 {
            self.invalidate();
        }
        let weight = self.frames.min(SAMPLE_COUNT - 1) as f32
            / (self.frames.min(SAMPLE_COUNT - 1) + 1) as f32;
        (moving, if moving { weight.min(0.65) } else { weight })
    }
    fn submitted(&mut self, vp: Mat4) {
        self.previous = Some(vp);
        self.samples = (self.samples + 1).min(SAMPLE_COUNT);
        self.frames = (self.frames + 1).min(SAMPLE_COUNT);
        self.phase = (self.phase + 1) % SAMPLE_COUNT;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniform {
    inverse_jittered: [[f32; 4]; 4],
    current: [[f32; 4]; 4],
    previous: [[f32; 4]; 4],
    // History blend weight, valid history, camera motion, padding.
    options: [f32; 4],
}

/// Size-independent GPU objects survive resize and mode changes. Keeping only
/// this small kernel while TAA is disabled does not retain history textures.
pub(super) struct TemporalKernel {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
}
impl TemporalKernel {
    pub fn new(device: &wgpu::Device) -> Self {
        let entries = (0..6)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: match binding {
                    0 | 1 => wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    2 => wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    3 => wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    4 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    _ => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                },
                count: None,
            })
            .collect::<Vec<_>>();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("TAA inputs"),
            entries: &entries,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("TAA reprojection and clipping"),
            source: wgpu::ShaderSource::Wgsl(include_str!("temporal.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("TAA"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("TAA resolve"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R32Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: Default::default(),
            }),
            multiview: None,
            cache: None,
        });
        Self {
            layout,
            sampler,
            pipeline,
        }
    }
}

pub(super) struct Temporal {
    pub color: [wgpu::TextureView; 2],
    depth: [wgpu::TextureView; 2],
    groups: [wgpu::BindGroup; 2],
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    state: HistoryState,
    pending: Uniform,
    pub output: usize,
    size: [u32; 2],
}
impl Temporal {
    pub fn new(
        device: &wgpu::Device,
        kernel: &TemporalKernel,
        scene: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Self {
        let color = std::array::from_fn(|_| {
            super::effects::Effects::target(
                device,
                wgpu::TextureFormat::Rgba16Float,
                size[0],
                size[1],
            )
        });
        let history_depth = std::array::from_fn(|_| {
            super::effects::Effects::target(device, wgpu::TextureFormat::R32Float, size[0], size[1])
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("TAA reprojection"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let groups = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("TAA ping pong history"),
                layout: &kernel.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(scene),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&color[1 - i]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&history_depth[1 - i]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&kernel.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            })
        });
        Self {
            color,
            depth: history_depth,
            groups,
            pipeline: kernel.pipeline.clone(),
            uniform,
            state: HistoryState::default(),
            pending: Uniform::zeroed(),
            output: 1,
            size,
        }
    }
    pub fn invalidate(&mut self) {
        self.state.invalidate();
    }
    pub fn samples(&self) -> u32 {
        self.state.samples
    }
    pub fn pending(&self) -> bool {
        self.samples() < SAMPLE_COUNT
    }
    pub fn prepare(&mut self, vp: Mat4) -> Mat4 {
        let (moving, weight) = self.state.prepare(vp);
        let render_vp = jittered(vp, jitter(self.state.phase), self.size);
        self.pending = Uniform {
            inverse_jittered: render_vp.inverse().to_cols_array_2d(),
            current: vp.to_cols_array_2d(),
            previous: self.state.previous.unwrap_or(vp).to_cols_array_2d(),
            options: [
                weight,
                if self.state.frames > 0 { 1. } else { 0. },
                if moving { 1. } else { 0. },
                0.,
            ],
        };
        render_vp
    }
    pub fn resolve(&mut self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder) {
        let output = 1 - self.output;
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.pending));
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("temporal history resolve"),
                color_attachments: &[
                    attachment(&self.color[output]),
                    attachment(&self.depth[output]),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.groups[output], &[]);
            pass.draw(0..3, 0..1);
        }
        self.output = output;
        self.state
            .submitted(Mat4::from_cols_array_2d(&self.pending.current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampling_is_centered_bounded_and_changes_every_frame() {
        let samples = (0..SAMPLE_COUNT).map(jitter).collect::<Vec<_>>();
        assert!(samples.iter().copied().sum::<Vec2>().length() < 1e-6);
        assert!(samples.iter().all(|s| s.abs().max_element() < 0.5));
        assert!(samples.windows(2).all(|s| s[0] != s[1]));
        assert_eq!(jitter(0), jitter(SAMPLE_COUNT));
    }
    #[test]
    fn history_settles_only_after_submitted_frames_and_resets_on_change() {
        let mut s = HistoryState::default();
        for _ in 0..3 {
            s.prepare(Mat4::IDENTITY);
        }
        assert_eq!(s.samples, 0); // A timed-out swapchain frame contributes nothing.
        for _ in 0..SAMPLE_COUNT {
            s.prepare(Mat4::IDENTITY);
            s.submitted(Mat4::IDENTITY);
        }
        assert_eq!(s.samples, SAMPLE_COUNT);
        let moved = Mat4::from_translation(Vec3::X * 0.01);
        assert!(s.prepare(moved).0);
        assert_eq!(s.samples, 0);
        s.submitted(moved);
        assert_eq!(s.samples, 1);
        s.invalidate();
        assert_eq!(s.prepare(moved).1, 0.);
        assert_eq!(s.phase, 0);
    }
    #[test]
    fn static_camera_reprojection_does_not_cancel_subpixel_sampling() {
        let vp = Mat4::perspective_rh(42_f32.to_radians(), 1.7, 0.03, 200.)
            * Mat4::look_at_rh(Vec3::new(4., 2., 6.), Vec3::ZERO, Vec3::Y);
        let pixel = Vec3::new(0.2, -0.3, 0.98);
        for phase in 0..SAMPLE_COUNT {
            let jvp = jittered(vp, jitter(phase), [1280, 720]);
            let world = jvp.inverse().project_point3(pixel);
            let current = vp.project_point3(world);
            let previous = vp.project_point3(world);
            let history_ndc = pixel.truncate() + previous.truncate() - current.truncate();
            assert!((history_ndc - pixel.truncate()).length() < 1e-6);
            assert!((jvp.project_point3(world) - pixel).length() < 1e-4);
        }
    }
    #[test]
    fn supersampling_respects_scale_device_and_memory_bounds() {
        for mode in [Mode::Off, Mode::Fxaa, Mode::Taa, Mode::Taa2x, Mode::Ssaa] {
            let factor = if mode.supersampled() { 2 } else { 1 };
            assert_eq!(
                render_extent([1281, 721], 50, mode, 8192),
                ([640 * factor, 360 * factor], false)
            );
            let (extent, limited) = render_extent([8192, 8192], 100, mode, 4096);
            assert!(limited && extent[0] <= 4096 && extent[1] <= 4096);
            let bytes =
                u64::from(extent[0]) * u64::from(extent[1]) * if mode.temporal() { 32 } else { 8 };
            assert!(bytes <= 256 * 1024 * 1024);
        }
        assert_eq!(render_extent([0, 0], 50, Mode::Taa, 8192), ([1, 1], false));
    }
}
