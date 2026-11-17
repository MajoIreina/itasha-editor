use super::*;
use crate::interaction::{Geometry, ScreenControlFrame};
use glam::Vec2;
use itasha_contracts::Layer;
use std::collections::HashMap;
pub(super) struct Artwork {
    pipeline: wgpu::RenderPipeline,
    controls: wgpu::RenderPipeline,
    pub layers: Vec<Layer>,
    pub selected: Option<String>,
    textures: HashMap<String, ArtTexture>,
    pub geometry: Option<Geometry>,
    pub control_points: Vec<Vec2>,
    pub control_frame: Option<ScreenControlFrame>,
    pub control_basis: Option<(f32, Vec2, Vec2)>,
    coverage: HashMap<String, CoverageGpu>,
}
struct CoverageGpu {
    fragments: Vec<itasha_surface_map::CoverageFragment>,
    vertices: wgpu::Buffer,
    count: u32,
}
struct ArtTexture {
    gpu: DecalGpu,
    uniform: wgpu::Buffer,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}
impl Artwork {
    pub fn has_texture(&self, id: &str) -> bool {
        self.textures.contains_key(id)
    }
    pub fn clear(&mut self) {
        self.textures.clear();
        self.coverage.clear();
        self.geometry = None;
        self.control_points.clear();
        self.control_frame = None;
        self.control_basis = None;
    }
    pub fn new(
        device: &wgpu::Device,
        camera: &wgpu::BindGroupLayout,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("surface artwork"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("../environment.wgsl"),
                    "\n",
                    include_str!("artwork.wgsl")
                )
                .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("artwork"),
            bind_group_layouts: &[camera, layout],
            push_constant_ranges: &[],
        });
        let depth = Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: Default::default(),
            bias: wgpu::DepthBiasState {
                constant: -1,
                slope_scale: -1.,
                clamp: 0.,
            },
        });
        let pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {label:Some("paint-only artwork"),layout:Some(&pipeline_layout),vertex:wgpu::VertexState {module:&shader,entry_point:Some("vs"),buffers:&[wgpu::VertexBufferLayout {array_stride:std::mem::size_of::<Vertex>() as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x2]}],compilation_options:Default::default()},primitive:Default::default(),depth_stencil:depth.clone(),multisample:Default::default(),fragment:Some(wgpu::FragmentState {module:&shader,entry_point:Some("fs"),targets:&[Some(wgpu::ColorTargetState {format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})],compilation_options:Default::default()}),multiview:None,cache:None});
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("selection handles"),source:wgpu::ShaderSource::Wgsl("struct Out {@builtin(position) p:vec4<f32>}; @vertex fn vs(@location(0) p:vec2<f32>)->Out {var o:Out;o.p=vec4(p,0.,1.);return o;} @fragment fn fs()->@location(0) vec4<f32>{return vec4(0.04,0.48,1.,1.);}".into())});
        let controls = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("eight handles and rotation"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2],
                }],
                compilation_options: Default::default(),
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            controls,
            layers: vec![],
            selected: None,
            textures: HashMap::new(),
            geometry: None,
            control_points: vec![],
            coverage: HashMap::new(),
            control_frame: None,
            control_basis: None,
        }
    }
    pub fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: Vec<Layer>,
        selected: Option<String>,
    ) {
        let old = std::mem::replace(&mut self.layers, layers);
        self.selected = selected;
        self.textures
            .retain(|id, _| self.layers.iter().any(|l| l.id.to_string() == *id));
        self.coverage
            .retain(|id, _| self.layers.iter().any(|l| l.id.to_string() == *id));
        for layer in &self.layers {
            let id = layer.id.to_string();
            if (!self.coverage.contains_key(&id)
                || old
                    .iter()
                    .find(|l| l.id == layer.id)
                    .is_none_or(|l| l.placement != layer.placement))
                && let Some(geometry) = &self.geometry
            {
                let p = &layer.placement;
                let fragments = geometry.topology.coverage(
                    &p.surface_id,
                    itasha_surface_map::CoverageRect {
                        center: Vec2::new(p.x_mm as f32, p.y_mm as f32),
                        size: Vec2::new(p.width_mm as f32, p.height_mm as f32),
                        rotation_rad: (p.rotation_deg as f32).to_radians(),
                    },
                    p.cross_surface,
                );
                let vertices: Vec<Vertex> = fragments
                    .iter()
                    .flat_map(|f| {
                        let w: Vec<_> = f.world.iter().copied().map(Vec3::from_array).collect();
                        let normal = (w[1] - w[0])
                            .cross(w[2] - w[0])
                            .normalize_or_zero()
                            .to_array();
                        (0..3).map(move |i| Vertex {
                            position: f.world[i],
                            normal,
                            uv: f.image_uv[i],
                            uv1: f.paint_uv[i],
                        })
                    })
                    .collect();
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("bounded paint fragments"),
                    contents: if vertices.is_empty() {
                        &[0; 40]
                    } else {
                        bytemuck::cast_slice(&vertices)
                    },
                    usage: wgpu::BufferUsages::VERTEX,
                });
                self.coverage.insert(
                    id.clone(),
                    CoverageGpu {
                        count: vertices.len() as u32,
                        vertices: buffer,
                        fragments,
                    },
                );
            }
            if let Some(texture) = self.textures.get(&id) {
                let mut uniform = [[0_f32; 4]; 6];
                uniform[4][2] = if layer.placement.flip_x { 1. } else { 0. };
                uniform[5] = [
                    0.,
                    layer.opacity as f32,
                    if self.selected.as_deref() == Some(&id) {
                        1.
                    } else {
                        0.
                    },
                    0.,
                ];
                queue.write_buffer(&texture.uniform, 0, bytemuck::cast_slice(&uniform));
            }
        }
    }

    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        id: String,
        dimensions: [u32; 2],
        pixels: Vec<u8>,
    ) -> Result<(), String> {
        let [width, height] = dimensions;
        let retained = self
            .textures
            .iter()
            .filter(|(key, _)| **key != id)
            .map(|(_, t)| t.pixels.len())
            .sum::<usize>();
        if retained + pixels.len() > 128 * 1024 * 1024 {
            return Err("素材预览纹理超过 128 MiB 预算，请合并或减少素材。".into());
        }
        let gpu = Gpu::make_decal(device, queue, layout, [[0.; 4]; 3], width, height, &pixels);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer projector"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = gpu.texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer artwork"),
            layout,
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
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.textures.insert(
            id,
            ArtTexture {
                gpu: DecalGpu { group, ..gpu },
                uniform,
                pixels,
                width,
                height,
            },
        );
        self.sync(device, queue, self.layers.clone(), self.selected.clone());
        Ok(())
    }
    pub fn hit_layer(&self, hit: &crate::interaction::Hit) -> Option<&Layer> {
        self.hit_layer_at(hit).map(|(layer, _)| layer)
    }

    pub fn hit_layer_at(&self, hit: &crate::interaction::Hit) -> Option<(&Layer, Vec2)> {
        self.layers.iter().rev().find_map(|layer| {
            if !layer.visible || layer.opacity <= 0. {
                return None;
            }
            let id = layer.id.to_string();
            let Some(coverage) = self.coverage.get(&id) else {
                return None;
            };
            let Some(texture) = self.textures.get(&id) else {
                return None;
            };
            coverage
                .fragments
                .iter()
                .filter(|f| f.triangle == hit.triangle_id)
                .find_map(|f| {
                    let Some(b) = crate::interaction::barycentric_world(
                        hit.world,
                        [f.world[0], f.world[1], f.world[2]].map(Vec3::from_array),
                    ) else {
                        return None;
                    };
                    if b.min_element() < -0.002 {
                        return None;
                    }
                    let corners =
                        [f.image_uv[0], f.image_uv[1], f.image_uv[2]].map(Vec2::from_array);
                    let uv = corners[0] * b.x + corners[1] * b.y + corners[2] * b.z;
                    let mut sample_uv = uv;
                    if layer.placement.flip_x {
                        sample_uv.x = 1. - sample_uv.x;
                    }
                    let x = (sample_uv.x.clamp(0., 1.) * (texture.width - 1) as f32) as usize;
                    let y = (sample_uv.y.clamp(0., 1.) * (texture.height - 1) as f32) as usize;
                    (texture.pixels[(y * texture.width as usize + x) * 4 + 3] > 12)
                        .then_some((layer, uv))
                })
        })
    }

    pub fn controls(&mut self, vp: Mat4, width: u32, height: u32, scale: f32) -> Vec<[f32; 2]> {
        self.control_points.clear();
        self.control_frame = None;
        let mut vertices = vec![];
        let Some(layer) = self
            .layers
            .iter()
            .find(|l| Some(l.id.to_string()) == self.selected && l.visible)
        else {
            return vertices;
        };
        let Some(geometry) = &self.geometry else {
            return vertices;
        };
        let p = &layer.placement;
        let Some((anchor, chart_u, chart_v)) = geometry.anchor_frame(layer) else {
            return vertices;
        };
        // Hide controls when the anchor is behind opaque geometry or the camera.
        let inv = vp.inverse();
        let eye = inv.project_point3(Vec3::new(0., 0., 0.));
        let ray = anchor - eye;
        if let Some(hit) = geometry.pick(eye, ray.normalize())
            && hit.world.distance(anchor) > 0.2
        {
            return vertices;
        }
        let project = |world: Vec3| {
            let clip = vp * world.extend(1.);
            if clip.w <= 0.03 {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            (ndc.z >= 0. && ndc.z <= 1.).then_some(Vec2::new(
                (ndc.x * 0.5 + 0.5) * width as f32,
                (0.5 - ndc.y * 0.5) * height as f32,
            ))
        };
        let Some(origin) = project(anchor) else {
            return vec![];
        };
        let (sin, cos) = (p.rotation_deg as f32).to_radians().sin_cos();
        let (x, y) = if let Some((rotation, x, y)) = self.control_basis {
            // Keep the editing axes stable while the anchor crosses a chart
            // boundary. Rotation follows the frozen screen handedness.
            let angle = ((p.rotation_deg as f32 - rotation) * x.perp_dot(y)).to_radians();
            let turn = glam::Mat2::from_angle(angle);
            (turn * x, turn * y)
        } else {
            (
                project(anchor + (chart_u * cos + chart_v * sin) * 10.)
                    .map_or(Vec2::X, |v| v - origin),
                project(anchor + (chart_v * cos - chart_u * sin) * 10.)
                    .map_or(Vec2::Y, |v| v - origin),
            )
        };
        let Some(coverage) = self.coverage.get(&layer.id.to_string()) else {
            return vec![];
        };
        let projected = coverage
            .fragments
            .iter()
            .flat_map(|f| &f.world)
            .filter_map(|w| project(Vec3::from_array(*w)));
        let Some(frame) = ScreenControlFrame::fit(projected, x, y, 12. * scale) else {
            return vec![];
        };
        self.control_points = frame.points(26. * scale);
        self.control_frame = Some(frame);
        let mut line = |a: Vec2, b: Vec2, thickness: f32| {
            let d = (b - a).normalize_or_zero();
            let n = Vec2::new(-d.y, d.x) * thickness * 0.5;
            for v in [a + n, a - n, b + n, b + n, a - n, b - n] {
                vertices.push([v.x / width as f32 * 2. - 1., 1. - v.y / height as f32 * 2.]);
            }
        };
        for (a, b) in [(0, 2), (2, 4), (4, 6), (6, 0), (1, 8)] {
            line(self.control_points[a], self.control_points[b], 1.5 * scale);
        }
        if !layer.locked {
            for point in &self.control_points {
                line(
                    *point - Vec2::X * 4. * scale,
                    *point + Vec2::X * 4. * scale,
                    8. * scale,
                );
            }
        }
        vertices
    }
    pub fn render<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a GpuScene,
        camera: &'a wgpu::BindGroup,
    ) {
        let _ = scene;
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera, &[]);
        for layer in &self.layers {
            if !layer.visible || layer.opacity <= 0. {
                continue;
            }
            let id = layer.id.to_string();
            let (Some(texture), Some(coverage)) = (self.textures.get(&id), self.coverage.get(&id))
            else {
                continue;
            };
            pass.set_bind_group(1, &texture.gpu.group, &[]);
            pass.set_vertex_buffer(0, coverage.vertices.slice(..));
            pass.draw(0..coverage.count, 0..1);
        }
    }

    pub fn render_controls<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        pass.set_pipeline(&self.controls);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..count, 0..1);
    }
}
