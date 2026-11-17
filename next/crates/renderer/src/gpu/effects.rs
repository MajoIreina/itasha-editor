use super::temporal::{Mode, Temporal, TemporalKernel};
use wgpu::util::DeviceExt;

pub(super) struct Effects {
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    sky: wgpu::RenderPipeline,
    post: wgpu::RenderPipeline,
    mode: Mode,
    temporal: Option<Temporal>,
    temporal_kernel: Option<TemporalKernel>,
    history_groups: Option<[wgpu::BindGroup; 2]>,
    light: Option<bool>,
}
/// Display-only solar orbit: sunrise 06:00, noon 12:00, sunset 18:00.
pub fn sun(minutes: u16) -> [f32; 4] {
    let angle = (f32::from(minutes) / 1440. - 0.25) * std::f32::consts::TAU;
    let direction = glam::Vec3::new(angle.cos(), angle.sin(), 0.35).normalize();
    [
        direction.x,
        direction.y,
        direction.z,
        (direction.y * 6.).clamp(0., 1.),
    ]
}
pub fn daylight(minutes: u16) -> f32 {
    ((sun(minutes)[1] + 0.12) / 0.32).clamp(0., 1.)
}
impl Effects {
    pub(super) fn target(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        w: u32,
        h: u32,
    ) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("scaled scene color"),
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
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
    fn group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene resolve"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
            ],
        })
    }
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        camera: &wgpu::BindGroupLayout,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post processing"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("antialiasing resolve parameters"),
            contents: bytemuck::cast_slice(&[0_f32; 8]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let view = Self::target(device, format, 1, 1);
        let group = Self::group(device, &layout, &view, &sampler, &params);
        let create = |name, source: &str, bind: &wgpu::BindGroupLayout, depth| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(name),
                bind_group_layouts: &[bind],
                push_constant_ranges: &[],
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: Default::default(),
                depth_stencil: depth,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview: None,
                cache: None,
            })
        };
        let sky = create(
            "procedural skybox",
            concat!(
                include_str!("../environment.wgsl"),
                "\n",
                include_str!("sky.wgsl")
            ),
            camera,
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
        );
        let post = create("FXAA resolve", include_str!("post.wgsl"), &layout, None);
        Self {
            view,
            width: 1,
            height: 1,
            layout,
            group,
            sampler,
            params,
            sky,
            post,
            mode: Mode::Fxaa,
            temporal: None,
            temporal_kernel: None,
            history_groups: None,
            light: None,
        }
    }
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        w: u32,
        h: u32,
        mode: Mode,
        depth: &wgpu::TextureView,
    ) {
        if self.width == w && self.height == h && self.mode == mode {
            return;
        }
        // Release histories entirely when leaving a temporal mode.
        self.history_groups = None;
        self.temporal = None;
        if self.width != w || self.height != h {
            self.view = Self::target(device, format, w, h);
        }
        self.width = w;
        self.height = h;
        self.mode = mode;
        self.group = Self::group(
            device,
            &self.layout,
            &self.view,
            &self.sampler,
            &self.params,
        );
        if mode.temporal() {
            let kernel = self
                .temporal_kernel
                .get_or_insert_with(|| TemporalKernel::new(device));
            let temporal = Temporal::new(device, kernel, &self.view, depth, [w, h]);
            self.history_groups = Some(std::array::from_fn(|i| {
                Self::group(
                    device,
                    &self.layout,
                    &temporal.color[i],
                    &self.sampler,
                    &self.params,
                )
            }));
            self.temporal = Some(temporal);
        }
    }
    pub fn invalidate(&mut self) {
        if let Some(temporal) = &mut self.temporal {
            temporal.invalidate();
        }
    }
    pub fn temporal_samples(&self) -> u32 {
        self.temporal.as_ref().map_or(0, Temporal::samples)
    }
    pub fn temporal_pending(&self) -> bool {
        self.temporal.as_ref().is_some_and(Temporal::pending)
    }
    pub fn prepare(&mut self, vp: glam::Mat4, light: bool) -> glam::Mat4 {
        if self.light != Some(light) {
            self.invalidate();
            self.light = Some(light);
        }
        self.temporal
            .as_mut()
            .map_or(vp, |temporal| temporal.prepare(vp))
    }
    pub fn sky<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, camera: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.sky);
        pass.set_bind_group(0, camera, &[]);
        pass.draw(0..3, 0..1);
    }
    pub fn present(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        output_size: [u32; 2],
    ) {
        if let Some(temporal) = &mut self.temporal {
            temporal.resolve(queue, encoder);
        }
        let mode = if self.mode == Mode::Fxaa {
            1.
        } else if self.mode.supersampled()
            && (self.width > output_size[0] || self.height > output_size[1])
        {
            2.
        } else {
            0.
        };
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::cast_slice(&[
                1. / self.width as f32,
                1. / self.height as f32,
                mode,
                0.,
                1. / output_size[0] as f32,
                1. / output_size[1] as f32,
                0.,
                0.,
            ]),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("resolve scaled scene and FXAA"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.post);
        let group = match (&self.temporal, &self.history_groups) {
            (Some(temporal), Some(groups)) => &groups[temporal.output],
            _ => &self.group,
        };
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn solar_cycle_is_finite_periodic_and_dark_at_midnight() {
        assert_eq!(daylight(0), 0.);
        assert_eq!(daylight(720), 1.);
        assert!(sun(720)[1] > 0.9);
        assert!(sun(0)[1] < -0.9);
        for time in 0..1440 {
            let s = sun(time);
            assert!(s.iter().all(|v| v.is_finite()));
            assert!((0. ..=1.).contains(&s[3]));
        }
    }
}
