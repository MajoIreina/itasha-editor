mod artwork;
mod effects;
mod temporal;
use crate::assets::{Scene, SceneStats, Vertex};
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
pub use glam::Vec3;
use std::time::Instant;
use wgpu::util::DeviceExt;

fn ground_parameters(min: Vec3, max: Vec3) -> ([f32; 4], [f32; 4]) {
    let span = max - min;
    let center = (min + max) * 0.5;
    (
        [center.x, min.y, center.z, 1.],
        [
            (span.x * 0.5).max(0.1) + 0.18,
            (span.z * 0.5).max(0.1) + 0.14,
            0.,
            0.,
        ],
    )
}

pub struct Camera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub radius: f32,
    home: Vec3,
    ground: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::new(0., 0.7, 0.),
            yaw: -2.25,
            pitch: 0.23,
            distance: 7.5,
            radius: 2.8,
            home: Vec3::new(0., 0.7, 0.),
            ground: 0.,
        }
    }
}
impl Camera {
    pub fn reset(&mut self, min: Vec3, max: Vec3) {
        self.target = (min + max) * 0.5;
        self.radius = ((max - min).length() * 0.5).max(0.1);
        self.home = self.target;
        self.ground = min.y;
        self.distance = self.radius * 2.7;
        self.yaw = -2.25;
        self.pitch = 0.23;
    }
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.006;
        self.pitch = (self.pitch + dy * 0.006).clamp(0.01, 1.45);
    }
    pub fn zoom(&mut self, delta: f32) {
        self.distance =
            (self.distance * (-delta * 0.0015).exp()).clamp(self.radius * 0.45, self.radius * 4.);
    }
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let right = Vec3::new(self.yaw.cos(), 0., -self.yaw.sin());
        self.target += (-right * dx + Vec3::Y * dy) * self.distance * 0.001;
        self.constrain();
    }
    pub fn constrain(&mut self) {
        self.pitch = self.pitch.clamp(0.01, 1.45);
        self.distance = self.distance.clamp(self.radius * 0.3, self.radius * 4.);
        self.target = self.target.clamp(
            self.home - Vec3::splat(self.radius),
            self.home + Vec3::splat(self.radius),
        );
        self.target.y = self.target.y.max(self.ground + 0.05);
    }
    pub fn eye(&self, aspect: f32) -> Vec3 {
        self.target
            + Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            ) * self.distance
                * (1.0 / aspect.max(0.05)).clamp(1.0, 2.5)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ground_tracks_model_contact_height_and_footprint_after_translation() {
        let min = Vec3::new(-1.1, 0.35, -2.8);
        let max = Vec3::new(1.1, 2.4, 2.8);
        let offset = Vec3::new(12., -4., 9.);
        let (ground, footprint) = ground_parameters(min, max);
        let (translated, moved_footprint) = ground_parameters(min + offset, max + offset);
        assert_eq!(ground, [0., min.y, 0., 1.]);
        assert!((translated[0] - offset.x).abs() < 1e-5);
        assert!((translated[1] - min.y - offset.y).abs() < 1e-5);
        assert!((translated[2] - offset.z).abs() < 1e-5);
        for axis in 0..2 {
            assert!((footprint[axis] - moved_footprint[axis]).abs() < 1e-5);
        }
        // The wider longitudinal dimension stays on Z for this model; no
        // SU7-specific orientation or metre-scale dimensions are assumed.
        assert!(footprint[1] > footprint[0]);
        assert!(footprint[0] > (max.x - min.x) * 0.5);
        assert!(footprint[1] > (max.z - min.z) * 0.5);
        let (_, small) = ground_parameters(Vec3::ZERO, Vec3::splat(0.01));
        assert!(small[0] > 0. && small[1] > 0.);
    }
    #[test]
    fn camera_limits_zoom_orbit_and_pan_above_ground() {
        let mut camera = Camera::default();
        camera.reset(Vec3::new(-2.5, 0., -1.), Vec3::new(2.5, 1.6, 1.));
        camera.orbit(0., -1e6);
        camera.zoom(-1e6);
        camera.pan(1e6, -1e6);
        camera.constrain();
        assert!(camera.pitch >= 0.01);
        assert!(camera.distance <= camera.radius * 4.);
        assert!(camera.target.x.abs() <= camera.radius);
        for aspect in [0.1, 0.5, 1., 2.] {
            assert!(camera.eye(aspect).y > 0.);
        }
        camera.orbit(0., 1e6);
        camera.zoom(1e6);
        assert!(camera.pitch <= 1.45);
        assert!(camera.distance >= camera.radius * 0.3);
    }
    #[test]
    fn reset_keeps_vehicle_bounds_inside_wide_and_narrow_viewports() {
        let min = Vec3::new(-2.5, 0., -1.);
        let max = Vec3::new(2.5, 1.6, 1.);
        let mut camera = Camera::default();
        camera.reset(min, max);
        for aspect in [0.4, 0.7, 1., 2.1] {
            let matrix = Mat4::perspective_rh(42_f32.to_radians(), aspect, 0.03, 200.)
                * Mat4::look_at_rh(camera.eye(aspect), camera.target, Vec3::Y);
            for x in [min.x, max.x] {
                for y in [min.y, max.y] {
                    for z in [min.z, max.z] {
                        let clip = matrix.project_point3(Vec3::new(x, y, z));
                        assert!(
                            clip.x.abs() < 1. && clip.y.abs() < 1.,
                            "clipped corner at aspect {aspect}: {clip}"
                        );
                    }
                }
            }
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    vp: [[f32; 4]; 4],
    eye: [f32; 4],
    inverse_vp: [[f32; 4]; 4],
    sun: [f32; 4],
    environment: [f32; 4],
    ground: [f32; 4],
    footprint: [f32; 4],
}
struct Draw {
    surface: Option<String>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    material: usize,
    center: Vec3,
}
struct GpuMaterial {
    group: wgpu::BindGroup,
    transparent: bool,
}
struct GpuScene {
    decals: std::collections::HashMap<String, DecalGpu>,
    draws: Vec<Draw>,
    materials: Vec<GpuMaterial>,
    _textures: Vec<wgpu::Texture>,
}
struct DecalGpu {
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
    projection: [[f32; 4]; 3],
}
pub struct Gpu {
    artwork: artwork::Artwork,
    effects: effects::Effects,
    pub performance: itasha_contracts::PerformanceSettings,
    pub environment: itasha_contracts::SceneSettings,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    camera_buffer: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    material_layout: wgpu::BindGroupLayout,
    decal_layout: wgpu::BindGroupLayout,
    empty_decal: DecalGpu,
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    depth: wgpu::TextureView,
    scene: Option<GpuScene>,
    pub camera: Camera,
    pub adapter: String,
    pub backend: String,
    pub software: bool,
    pub frames: u64,
    pub last_submit_ms: f64,
    capture_next: bool,
    pub control_scale: f32,
    ground: [f32; 4],
    footprint: [f32; 4],
}
impl Gpu {
    pub async fn new(target: impl Into<wgpu::SurfaceTarget<'static>>) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let surface = instance.create_surface(target).map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Itasha native renderer"),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .ok_or("GPU 不支持 sRGB 视口。")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | if std::env::var_os("ITASHA_TEST_DATA_DIR").is_some()
                    && caps.usages.contains(wgpu::TextureUsages::COPY_SRC)
                {
                    wgpu::TextureUsages::COPY_SRC
                } else {
                    wgpu::TextureUsages::empty()
                },
            format,
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let mut entries = vec![
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
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ];
        for binding in 2..6 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PBR material"),
            entries: &entries,
        });
        let decal_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("part projection"),
            entries: &[
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
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let empty_decal =
            Self::make_decal(&device, &queue, &decal_layout, [[0.; 4]; 3], 1, 1, &[0; 4]);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("studio"),
            bind_group_layouts: &[&camera_layout, &material_layout, &decal_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("studio PBR"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("environment.wgsl"),
                    "\n",
                    include_str!("studio.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = |transparent| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
            label:Some(if transparent{"transparent"}else{"opaque"}),layout:Some(&layout),
            vertex:wgpu::VertexState{module:&shader,entry_point:Some("vs"),buffers:&[wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<Vertex>() as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x2]}],compilation_options:Default::default()},
            primitive:wgpu::PrimitiveState{cull_mode:None,..Default::default()},
            depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:!transparent,depth_compare:wgpu::CompareFunction::LessEqual,stencil:Default::default(),bias:Default::default()}),
            multisample:Default::default(),fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs"),targets:&[Some(wgpu::ColorTargetState{format,blend:if transparent{Some(wgpu::BlendState::ALPHA_BLENDING)}else{None},write_mask:wgpu::ColorWrites::ALL})],compilation_options:Default::default()}),multiview:None,cache:None})
        };
        let opaque = pipeline(false);
        let transparent = pipeline(true);
        let artwork = artwork::Artwork::new(&device, &camera_layout, &decal_layout, format);
        let depth = Self::depth(&device, 1, 1);
        let effects = effects::Effects::new(&device, format, &camera_layout);
        Ok(Self {
            artwork,
            effects,
            performance: Default::default(),
            environment: Default::default(),
            decal_layout,
            empty_decal,
            surface,
            device,
            queue,
            config,
            camera_buffer,
            camera_group,
            material_layout,
            opaque,
            transparent,
            depth,
            scene: None,
            camera: Camera::default(),
            adapter: info.name,
            backend: format!("{:?}", info.backend),
            software: info.device_type == wgpu::DeviceType::Cpu,
            frames: 0,
            last_submit_ms: 0.,
            capture_next: false,
            control_scale: 1.,
            ground: [0., 0., 0., 0.],
            footprint: [0.5, 0.5, 0., 0.],
        })
    }
    fn depth(device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
    pub fn resize(&mut self, w: u32, h: u32) {
        if self.config.width != w || self.config.height != h {
            self.artwork.control_points.clear();
        }
        if w == 0 || h == 0 {
            return;
        }
        let w = w.min(8192);
        let h = h.min(8192);
        if w == self.config.width && h == self.config.height {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.effects.invalidate();
        self.surface.configure(&self.device, &self.config);
        self.resize_targets();
    }
    pub fn render_size(&self) -> (u32, u32) {
        (self.effects.width, self.effects.height)
    }
    pub fn temporal_samples(&self) -> u32 {
        self.effects.temporal_samples()
    }
    pub fn temporal_pending(&self) -> bool {
        self.scene.is_some() && self.effects.temporal_pending()
    }
    pub fn render_scale_limited(&self) -> bool {
        temporal::render_extent(
            [self.config.width, self.config.height],
            self.performance.render_scale,
            temporal::Mode::parse(&self.performance.antialiasing),
            self.device.limits().max_texture_dimension_2d,
        )
        .1
    }
    pub fn invalidate_history(&mut self) {
        self.effects.invalidate();
        self.capture_next = true;
    }
    fn resize_targets(&mut self) {
        let mode = temporal::Mode::parse(&self.performance.antialiasing);
        let ([w, h], _) = temporal::render_extent(
            [self.config.width, self.config.height],
            self.performance.render_scale,
            mode,
            self.device.limits().max_texture_dimension_2d,
        );
        if self.effects.width != w || self.effects.height != h {
            self.depth = Self::depth(&self.device, w, h);
        }
        self.effects
            .resize(&self.device, self.config.format, w, h, mode, &self.depth);
    }
    pub fn configure(
        &mut self,
        performance: itasha_contracts::PerformanceSettings,
        environment: itasha_contracts::SceneSettings,
    ) {
        self.performance = performance;
        self.environment = environment;
        self.resize_targets();
        self.effects.invalidate();
        self.capture_next = true;
    }
    pub fn clear(&mut self) {
        self.effects.invalidate();
        self.artwork.clear();
        self.scene = None;
        self.ground[3] = 0.;
        self.frames = 0;
    }
    pub fn upload(&mut self, scene: Scene) -> (SceneStats, f64) {
        self.effects.invalidate();
        let started = Instant::now();
        self.artwork.geometry = Some(crate::interaction::Geometry::new(&scene));
        // Keep the soft contact shadow proportional to the actual model instead
        // of tying the ground appearance to a particular vehicle's dimensions.
        (self.ground, self.footprint) = ground_parameters(scene.min, scene.max);
        self.camera.reset(scene.min, scene.max);
        let mut textures = vec![];
        let mut views = vec![];
        for texture in scene.textures {
            let size = wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            };
            let gpu = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("model texture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: if texture.srgb {
                    wgpu::TextureFormat::Rgba8UnormSrgb
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                },
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &texture.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(texture.width * 4),
                    rows_per_image: Some(texture.height),
                },
                size,
            );
            views.push(gpu.create_view(&Default::default()));
            textures.push(gpu);
        }
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let materials = scene
            .materials
            .into_iter()
            .map(|material| {
                let uniform = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("PBR"),
                        contents: bytemuck::bytes_of(&material.uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                let mut entries = vec![
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ];
                for (i, texture) in material.textures.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry {
                        binding: i as u32 + 2,
                        resource: wgpu::BindingResource::TextureView(&views[*texture]),
                    });
                }
                GpuMaterial {
                    group: self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("model material"),
                        layout: &self.material_layout,
                        entries: &entries,
                    }),
                    transparent: material.transparent,
                }
            })
            .collect();
        let mut mesh_surfaces = std::collections::HashMap::new();
        let mut decals = std::collections::HashMap::new();
        for surface in scene.surfaces {
            mesh_surfaces.insert(surface.mesh_id, surface.info.id.clone());
            decals.insert(
                surface.info.id,
                Self::make_decal(
                    &self.device,
                    &self.queue,
                    &self.decal_layout,
                    surface.projection,
                    1,
                    1,
                    &[0; 4],
                ),
            );
        }
        let draws = scene
            .meshes
            .into_iter()
            .filter(|mesh| !mesh.indices.is_empty())
            .map(|mesh| Draw {
                surface: mesh_surfaces.get(&mesh.id).cloned(),
                vertices: self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&mesh.id),
                        contents: bytemuck::cast_slice(&mesh.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                indices: self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("indices"),
                        contents: bytemuck::cast_slice(&mesh.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                count: mesh.indices.len() as u32,
                material: mesh.material,
                center: mesh.center,
            })
            .collect();
        self.scene = Some(GpuScene {
            decals,
            draws,
            materials,
            _textures: textures,
        });
        (scene.stats, started.elapsed().as_secs_f64() * 1000.)
    }
    fn make_decal(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        projection: [[f32; 4]; 3],
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> DecalGpu {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("part artwork"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("part projection"),
            contents: bytemuck::cast_slice(&projection),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("part artwork"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
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
        DecalGpu {
            texture,
            group,
            projection,
        }
    }
    pub fn focus_selected(&mut self) {
        let Some(layer) = self.selected_layer() else {
            return;
        };
        let Some(g) = &self.artwork.geometry else {
            return;
        };
        let p = &layer.placement;
        let Some(target) = g.world(&layer, glam::Vec2::new(p.x_mm as f32, p.y_mm as f32)) else {
            return;
        };
        let Some(info) = g.info(&p.surface_id) else {
            return;
        };
        let center = glam::Vec2::new(p.x_mm as f32, p.y_mm as f32);
        let u = g.world(&layer, center + glam::Vec2::X).unwrap_or(target) - target;
        let v = g.world(&layer, center + glam::Vec2::Y).unwrap_or(target) - target;
        let mut normal = u.cross(v).normalize_or_zero();
        if normal.dot(Vec3::from_slice(&info.projection[2][..3])) < 0. {
            normal = -normal;
        }
        self.camera.target = target;
        self.camera.distance = (p.width_mm.max(p.height_mm) as f32 / 1000. * 3.2).clamp(0.8, 8.);
        self.camera.yaw = if normal.y.abs() > 0.6 {
            -2.25
        } else {
            normal.x.atan2(normal.z)
        };
        self.camera.pitch = if normal.y.abs() > 0.6 { 1.15 } else { 0.1 };
        self.effects.invalidate();
        self.capture_next = true;
    }
    pub fn control_points(&self) -> Vec<[f32; 2]> {
        self.artwork
            .control_points
            .iter()
            .map(|p| p.to_array())
            .collect()
    }
    pub fn selected_id(&self) -> Option<String> {
        self.artwork.selected.clone()
    }
    pub fn painted_parts(&self) -> Vec<String> {
        self.artwork
            .layers
            .iter()
            .filter(|l| l.visible && l.opacity > 0.)
            .filter(|l| self.artwork.has_texture(&l.id.to_string()))
            .map(|l| l.placement.surface_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn editor(&mut self, layers: Vec<itasha_contracts::Layer>, selected: Option<String>) {
        self.effects.invalidate();
        self.artwork
            .sync(&self.device, &self.queue, layers, selected);
        self.capture_next = true;
    }
    pub fn artwork_texture(
        &mut self,
        id: String,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<(), String> {
        self.artwork.upload(
            &self.device,
            &self.queue,
            &self.decal_layout,
            id,
            [width, height],
            pixels,
        )?;
        self.effects.invalidate();
        self.capture_next = true;
        Ok(())
    }
    pub fn ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let aspect = self.config.width as f32 / self.config.height as f32;
        let vp = Mat4::perspective_rh(42_f32.to_radians(), aspect, 0.03, 200.)
            * Mat4::look_at_rh(self.camera.eye(aspect), self.camera.target, Vec3::Y);
        let inv = vp.inverse();
        let ndc = glam::Vec2::new(
            x / self.config.width as f32 * 2. - 1.,
            1. - y / self.config.height as f32 * 2.,
        );
        let near = inv.project_point3(ndc.extend(0.));
        let far = inv.project_point3(ndc.extend(1.));
        (near, (far - near).normalize())
    }
    pub fn pick(&self, x: f32, y: f32) -> Option<crate::interaction::Hit> {
        let (o, d) = self.ray(x, y);
        self.artwork.geometry.as_ref()?.pick(o, d)
    }
    pub fn layer_at(&self, x: f32, y: f32) -> Option<itasha_contracts::Layer> {
        self.artwork.hit_layer(&self.pick(x, y)?).cloned()
    }
    pub fn layer_point_at(&self, x: f32, y: f32) -> Option<(itasha_contracts::Layer, glam::Vec2)> {
        let hit = self.pick(x, y)?;
        self.artwork
            .hit_layer_at(&hit)
            .map(|(layer, uv)| (layer.clone(), uv))
    }
    pub fn probe(&self, x: f32, y: f32) -> serde_json::Value {
        let Some(hit) = self.pick(x, y) else {
            return serde_json::Value::Null;
        };
        let g = self.artwork.geometry.as_ref().unwrap();
        let info = &g.surfaces[hit.surface].info;
        let artwork = self.artwork.hit_layer_at(&hit);
        serde_json::json!({"surfaceId":info.id,"world":hit.world.to_array(),"localPoint":hit.paint_uv.to_array(),"triangleId":hit.triangle_id.stable_key(),"barycentric":hit.barycentric.to_array(),"layerId":artwork.map(|(l,_)|l.id.to_string()),"imageUv":artwork.map(|(_,uv)|uv.to_array())})
    }

    pub fn surface_at(&self, x: f32, y: f32) -> Option<String> {
        let hit = self.pick(x, y)?;
        Some(
            self.artwork.geometry.as_ref()?.surfaces[hit.surface]
                .info
                .id
                .clone(),
        )
    }
    pub fn selected_layer(&self) -> Option<itasha_contracts::Layer> {
        self.artwork
            .layers
            .iter()
            .find(|l| Some(l.id.to_string()) == self.artwork.selected)
            .cloned()
    }
    pub fn select(&mut self, id: Option<String>) {
        self.editor(self.artwork.layers.clone(), id);
    }
    pub fn handle_at(&self, x: f32, y: f32) -> Option<usize> {
        crate::interaction::nearest_handle(
            &self.artwork.control_points,
            glam::Vec2::new(x, y),
            12. * self.control_scale,
        )
    }
    pub fn control_frame(&self) -> Option<crate::interaction::ScreenControlFrame> {
        self.artwork.control_frame
    }
    pub fn freeze_control_basis(&mut self, layer: Option<&itasha_contracts::Layer>) {
        self.artwork.control_basis = layer.and_then(|layer| {
            self.artwork
                .control_frame
                .map(|frame| (layer.placement.rotation_deg as f32, frame.x, frame.y))
        });
    }
    pub fn rebase_control_angle(&mut self, angle: f32) {
        if let Some((stored, _, _)) = &mut self.artwork.control_basis {
            *stored = angle;
        }
    }
    pub fn edit_point(
        &self,
        layer: &itasha_contracts::Layer,
        x: f32,
        y: f32,
        plane: bool,
    ) -> Option<crate::interaction::SurfaceEditPoint> {
        let g = self.artwork.geometry.as_ref()?;
        let (o, d) = self.ray(x, y);
        g.edit_point(layer, o, d, plane)
    }
    pub fn preview(&mut self, layer: itasha_contracts::Layer) {
        let mut layers = self.artwork.layers.clone();
        if let Some(old) = layers.iter_mut().find(|l| l.id == layer.id) {
            *old = layer;
        }
        self.editor(layers, self.artwork.selected.clone());
    }
    pub fn constrain_layer(
        &self,
        previous: &itasha_contracts::Layer,
        next: &itasha_contracts::Layer,
    ) -> itasha_contracts::Layer {
        self.artwork
            .geometry
            .as_ref()
            .map(|g| g.constrain(previous, next))
            .unwrap_or_else(|| previous.clone())
    }
    pub fn update_decal(
        &mut self,
        id: &str,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), String> {
        self.effects.invalidate();
        self.capture_next = true;
        let scene = self.scene.as_mut().ok_or("模型尚未载入。")?;
        let decal = scene.decals.get_mut(id).ok_or("目标外板不存在。")?;
        if decal.texture.width() == width && decal.texture.height() == height {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &decal.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        } else {
            *decal = Self::make_decal(
                &self.device,
                &self.queue,
                &self.decal_layout,
                decal.projection,
                width,
                height,
                pixels,
            );
        }
        Ok(())
    }
    pub fn render(&mut self, light: bool) -> Result<(), String> {
        let start = Instant::now();
        self.camera.constrain();
        let eye = self
            .camera
            .eye(self.config.width as f32 / self.config.height as f32);
        let vp = Mat4::perspective_rh(
            42_f32.to_radians(),
            self.config.width as f32 / self.config.height as f32,
            0.03,
            200.,
        ) * Mat4::look_at_rh(eye, self.camera.target, Vec3::Y);
        let render_vp = self.effects.prepare(vp, light);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform {
                vp: render_vp.to_cols_array_2d(),
                eye: [eye.x, eye.y, eye.z, 1.],
                inverse_vp: render_vp.inverse().to_cols_array_2d(),
                sun: effects::sun(self.environment.time_minutes),
                environment: [
                    effects::daylight(self.environment.time_minutes),
                    if self.environment.skybox { 1. } else { 0. },
                    if light { 1. } else { 0. },
                    0.,
                ],
                ground: self.ground,
                footprint: self.footprint,
            }),
        );
        let controls = self.artwork.controls(
            vp,
            self.config.width,
            self.config.height,
            self.control_scale,
        );
        let control_buffer = (!controls.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("selection UI"),
                    contents: bytemuck::cast_slice(&controls),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let output = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let view = output.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let bg = if light { 0.88 } else { 0.032 };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.effects.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg,
                            g: bg,
                            b: bg,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.effects.sky(&mut pass, &self.camera_group);
            pass.set_bind_group(0, &self.camera_group, &[]);
            if let Some(scene) = &self.scene {
                let mut order: Vec<_> = scene.draws.iter().collect();
                order.sort_by(|a, b| {
                    let at = scene.materials[a.material].transparent;
                    let bt = scene.materials[b.material].transparent;
                    at.cmp(&bt).then_with(|| {
                        if at {
                            b.center
                                .distance_squared(eye)
                                .total_cmp(&a.center.distance_squared(eye))
                        } else {
                            a.material.cmp(&b.material)
                        }
                    })
                });
                for draw in order {
                    let material = &scene.materials[draw.material];
                    pass.set_pipeline(if material.transparent {
                        &self.transparent
                    } else {
                        &self.opaque
                    });
                    pass.set_bind_group(1, &material.group, &[]);
                    let decal = draw
                        .surface
                        .as_ref()
                        .and_then(|id| scene.decals.get(id))
                        .unwrap_or(&self.empty_decal);
                    pass.set_bind_group(2, &decal.group, &[]);
                    pass.set_vertex_buffer(0, draw.vertices.slice(..));
                    pass.set_index_buffer(draw.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..draw.count, 0, 0..1);
                }
                self.artwork.render(&mut pass, scene, &self.camera_group);
            }
        }
        let settling = self.temporal_pending();
        self.effects.present(
            &self.queue,
            &mut encoder,
            &view,
            [self.config.width, self.config.height],
        );
        if let Some(control_buffer) = &control_buffer {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("full resolution editing controls"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.artwork
                .render_controls(&mut pass, control_buffer, controls.len() as u32);
        }
        let capture = if (self.frames == 0 || self.capture_next || settling)
            && !self.temporal_pending()
            && self.scene.is_some()
            && self.config.usage.contains(wgpu::TextureUsages::COPY_SRC)
        {
            std::env::var_os("ITASHA_TEST_DATA_DIR").map(|root| {
                let stride = (self.config.width * 4).div_ceil(256) * 256;
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("test screenshot"),
                    size: u64::from(stride) * u64::from(self.config.height),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: &output.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(stride),
                            rows_per_image: Some(self.config.height),
                        },
                    },
                    wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                );
                (
                    buffer,
                    stride,
                    std::path::PathBuf::from(root).join("native-viewport.png"),
                )
            })
        } else {
            None
        };
        self.queue.submit(Some(encoder.finish()));
        output.present();
        self.capture_next &= self.temporal_pending();
        self.frames += 1;
        self.last_submit_ms = start.elapsed().as_secs_f64() * 1000.;
        // Readback is enabled only for explicitly isolated acceptance runs.
        if let Some((buffer, stride, path)) = capture {
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            self.device
                .poll(wgpu::PollType::Wait)
                .map_err(|e| e.to_string())?;
            rx.recv()
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            let mapped = buffer.slice(..).get_mapped_range();
            let mut rgba =
                Vec::with_capacity((self.config.width * self.config.height * 4) as usize);
            for row in mapped.chunks_exact(stride as usize) {
                rgba.extend_from_slice(&row[..(self.config.width * 4) as usize]);
            }
            if matches!(
                self.config.format,
                wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
            ) {
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
            }
            let temporary = path.with_extension("writing.png");
            image::save_buffer(
                &temporary,
                &rgba,
                self.config.width,
                self.config.height,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            std::fs::rename(temporary, path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
