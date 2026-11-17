//! CPU-only standard glTF ingestion. No vendor runtime or graphics API dependency.
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    time::Instant,
};

pub type Result<T> = std::result::Result<T, String>;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub uv1: [f32; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MaterialUniform {
    pub color: [f32; 4],
    // metallic, roughness, alpha cutoff (-1 for opaque/blend), normal scale
    pub params: [f32; 4],
    // emissive RGB, occlusion strength
    pub emission: [f32; 4],
    // clearcoat, clearcoat roughness, AO UV channel, reserved
    pub coat: [f32; 4],
}
pub struct Texture {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub srgb: bool,
}
pub struct Material {
    pub paint: bool,
    pub uniform: MaterialUniform,
    pub textures: [usize; 4],
    pub transparent: bool,
}
pub struct Mesh {
    pub id: String,
    pub source_mesh: String,
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub source_primitive: u32,
    pub source_faces: Vec<SourceFaceId>,
    pub material: usize,
    pub center: Vec3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceFaceId {
    pub triangle: u32,
    pub fragment: u16,
}
pub struct Scene {
    pub paint_connections: Vec<itasha_surface_map::EdgeConnection>,
    pub surfaces: Vec<crate::surface::SurfaceGeometry>,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub min: Vec3,
    pub max: Vec3,
    pub stats: SceneStats,
}
#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneStats {
    pub surface_fingerprint: String,
    pub triangles: usize,
    pub primitives: usize,
    pub textures: usize,
    pub source_bytes: u64,
    pub texture_bytes: usize,
    pub parse_ms: f64,
    pub fingerprint: String,
}
#[derive(Clone, PartialEq)]
pub struct Paint {
    pub color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub coat: f32,
}
#[derive(Clone, PartialEq)]
pub struct Plan {
    pub paint_map: Option<PathBuf>,
    pub surfaces: Vec<crate::surface::SurfacePlan>,
    pub root: PathBuf,
    pub modules: Vec<String>,
    pub paint: Option<Paint>,
    pub interior: Option<String>,
    pub texture_limit: u32,
}

/// Only package-relative paths. Canonicalization also rejects symlink escapes.
pub fn package_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("资源路径必须位于模型包内。".into());
    }
    let base = root
        .canonicalize()
        .map_err(|e| format!("模型目录不可读取：{e}"))?;
    let resolved = base
        .join(path)
        .canonicalize()
        .map_err(|_| format!("缺少模型资源：{relative}"))?;
    if !resolved.starts_with(base) {
        return Err("模型资源越出包目录。".into());
    }
    Ok(resolved)
}

struct Loader {
    scene: Scene,
    texture_keys: HashMap<String, usize>,
    digest: Sha256,
}
impl Loader {
    fn texture(&mut self, bytes: &[u8], srgb: bool, limit: u32) -> Result<usize> {
        let key = format!("{:x}:{srgb}:{limit}", Sha256::digest(bytes));
        if let Some(id) = self.texture_keys.get(&key) {
            return Ok(*id);
        }
        // Inspect dimensions before allocating untrusted image output.
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
        if w == 0 || h == 0 || w > 8192 || h > 8192 || u64::from(w) * u64::from(h) > 32_000_000 {
            return Err("贴图尺寸超出当前导入预算。".into());
        }
        let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
        let image = if w > limit || h > limit {
            image.resize(limit, limit, image::imageops::FilterType::Triangle)
        } else {
            image
        };
        let pixels = image.into_rgba8();
        let id = self.scene.textures.len();
        self.scene.stats.texture_bytes += pixels.len();
        if self.scene.stats.texture_bytes > 512 * 1024 * 1024 {
            return Err("已解码贴图超出 512 MiB 预算。".into());
        }
        self.scene.textures.push(Texture {
            width: pixels.width(),
            height: pixels.height(),
            pixels: pixels.into_raw(),
            srgb,
        });
        self.texture_keys.insert(key, id);
        Ok(id)
    }
    fn image(
        &mut self,
        image: gltf::Image<'_>,
        blob: &[u8],
        module_root: &Path,
        srgb: bool,
        limit: u32,
    ) -> Result<usize> {
        match image.source() {
            gltf::image::Source::View { view, .. } => {
                let bytes = blob
                    .get(view.offset()..view.offset() + view.length())
                    .ok_or("贴图数据越界。")?;
                self.texture(bytes, srgb, limit)
            }
            gltf::image::Source::Uri { uri, .. } => {
                let file = package_file(module_root, uri)?;
                if file.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
                    return Err("单张贴图文件过大。".into());
                }
                let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
                self.digest.update(&bytes);
                self.texture(&bytes, srgb, limit)
            }
        }
    }
    fn node(
        &mut self,
        node: gltf::Node<'_>,
        parent: Mat4,
        blob: &[u8],
        material_offset: usize,
        module: &str,
    ) -> Result<()> {
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if !transform.is_finite() || transform.determinant().abs() < 1e-12 {
            return Err("模型变换无效。".into());
        }
        let normal = Mat3::from_mat4(transform).inverse().transpose();
        if let Some(mesh) = node.mesh() {
            for (primitive_id, primitive) in mesh.primitives().enumerate() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    return Err("当前只接入三角面 glTF 图元。".into());
                }
                let reader = primitive.reader(|buffer| {
                    if buffer.index() == 0 {
                        Some(blob)
                    } else {
                        None
                    }
                });
                let positions: Vec<_> = reader.read_positions().ok_or("网格缺少位置。")?.collect();
                let normals: Vec<_> = reader.read_normals().ok_or("网格缺少法线。")?.collect();
                let uv: Vec<_> = reader
                    .read_tex_coords(0)
                    .map(|v| v.into_f32().collect())
                    .unwrap_or_else(|| vec![[0.; 2]; positions.len()]);
                let uv1: Vec<_> = reader
                    .read_tex_coords(1)
                    .map(|v| v.into_f32().collect())
                    .unwrap_or_else(|| uv.clone());
                if positions.len() != normals.len()
                    || positions.len() != uv.len()
                    || positions.len() != uv1.len()
                {
                    return Err("网格属性数量不一致。".into());
                }
                let mut vertices = Vec::with_capacity(positions.len());
                let mut center = Vec3::ZERO;
                for i in 0..positions.len() {
                    let p = transform.transform_point3(Vec3::from(positions[i]));
                    let n = (normal * Vec3::from(normals[i])).normalize_or_zero();
                    if !p.is_finite()
                        || !n.is_finite()
                        || n.length_squared() < 0.5
                        || !uv[i].iter().chain(uv1[i].iter()).all(|v| v.is_finite())
                    {
                        return Err("网格包含非法顶点、法线或 UV。".into());
                    }
                    self.scene.min = self.scene.min.min(p);
                    self.scene.max = self.scene.max.max(p);
                    center += p;
                    vertices.push(Vertex {
                        position: p.into(),
                        normal: n.into(),
                        uv: uv[i],
                        uv1: uv1[i],
                    });
                }
                let mut indices: Vec<_> = reader
                    .read_indices()
                    .map(|v| v.into_u32().collect())
                    .unwrap_or_else(|| (0..positions.len() as u32).collect());
                if indices.len() % 3 != 0 || indices.iter().any(|i| *i as usize >= vertices.len()) {
                    return Err("三角面索引越界。".into());
                }
                // Mirrored nodes need their winding corrected after baking transforms.
                if transform.determinant() < 0. {
                    for face in indices.chunks_exact_mut(3) {
                        face.swap(1, 2);
                    }
                }
                self.scene.stats.triangles += indices.len() / 3;
                if self.scene.stats.triangles > 3_000_000 {
                    return Err("模型超出当前 300 万三角面导入预算。".into());
                }
                self.scene.meshes.push(Mesh {
                    name: node.name().unwrap_or("").into(),
                    id: format!("{module}/node/{}/primitive/{primitive_id}", node.index()),
                    source_mesh: format!("{module}/node/{}/primitive/{primitive_id}", node.index()),
                    vertices,
                    source_primitive: primitive_id as u32,
                    source_faces: (0..indices.len() / 3)
                        .map(|triangle| SourceFaceId {
                            triangle: triangle as u32,
                            fragment: 0,
                        })
                        .collect(),
                    indices,
                    material: material_offset
                        + primitive.material().index().ok_or("图元缺少材质。")?,
                    center: center / positions.len().max(1) as f32,
                });
            }
        }
        for child in node.children() {
            self.node(child, transform, blob, material_offset, module)?;
        }
        Ok(())
    }
    fn module(&mut self, plan: &Plan, module: &str) -> Result<()> {
        let file = package_file(&plan.root, module)?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 128 * 1024 * 1024 {
            return Err("单个模型超出 128 MiB 预算。".into());
        }
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        self.scene.stats.source_bytes += bytes.len() as u64;
        self.digest.update(module.as_bytes());
        self.digest.update(&bytes);
        let gltf = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("GLB 校验失败：{e}"))?;
        if gltf
            .buffers()
            .any(|b| !matches!(b.source(), gltf::buffer::Source::Bin))
        {
            return Err("此阶段要求 GLB 内嵌几何缓冲。".into());
        }
        let blob = gltf.blob.as_deref().ok_or("GLB 缺少几何数据。")?;
        let offset = self.scene.materials.len();
        for material in gltf.materials() {
            let pbr = material.pbr_metallic_roughness();
            let extra: Value = material
                .extras()
                .as_ref()
                .map(|v| serde_json::from_str(v.get()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(Value::Null);
            let mut color = pbr.base_color_factor();
            let mut metallic = pbr.metallic_factor();
            let mut roughness = pbr.roughness_factor();
            let extension = material.extension_value("KHR_materials_clearcoat");
            let mut coat = extension
                .and_then(|v| v["clearcoatFactor"].as_f64())
                .unwrap_or(0.) as f32;
            let coat_rough = extension
                .and_then(|v| v["clearcoatRoughnessFactor"].as_f64())
                .unwrap_or(0.1) as f32;
            if extra["native_paint_switch"] == true
                && let Some(paint) = &plan.paint
            {
                color = paint.color;
                metallic = paint.metallic;
                roughness = paint.roughness;
                coat = paint.coat;
            }
            let mut textures = [0, 1, 2, 2]; // white sRGB, flat normal, white linear, white linear
            if let Some(tex) = pbr.base_color_texture() {
                textures[0] = self.image(
                    tex.texture().source(),
                    blob,
                    file.parent().unwrap(),
                    true,
                    plan.texture_limit,
                )?;
            }
            if extra["native_interior_switch"] == true
                && let Some(interior) = &plan.interior
            {
                let key = extra["source_material"]
                    .as_str()
                    .ok_or("内饰材质缺少来源键。")?;
                let path = package_file(&plan.root, &format!("textures/{key}_{interior}_MP.png"))?;
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                self.digest.update(&bytes);
                textures[0] = self.texture(&bytes, true, plan.texture_limit)?;
                color = [1.; 4];
            }
            if let Some(tex) = material.normal_texture() {
                textures[1] = self.image(
                    tex.texture().source(),
                    blob,
                    file.parent().unwrap(),
                    false,
                    plan.texture_limit,
                )?;
            }
            if let Some(tex) = material.occlusion_texture() {
                textures[2] = self.image(
                    tex.texture().source(),
                    blob,
                    file.parent().unwrap(),
                    false,
                    plan.texture_limit,
                )?;
            }
            if let Some(tex) = pbr.metallic_roughness_texture() {
                textures[3] = self.image(
                    tex.texture().source(),
                    blob,
                    file.parent().unwrap(),
                    false,
                    plan.texture_limit,
                )?;
            }
            let e = material.emissive_factor();
            self.scene.materials.push(Material {
                paint: extra["native_paint_switch"] == true,
                uniform: MaterialUniform {
                    color,
                    params: [
                        metallic,
                        roughness.max(0.04),
                        if material.alpha_mode() == gltf::material::AlphaMode::Mask {
                            material.alpha_cutoff().unwrap_or(0.5)
                        } else {
                            -1.
                        },
                        material.normal_texture().map(|t| t.scale()).unwrap_or(1.),
                    ],
                    emission: [
                        e[0],
                        e[1],
                        e[2],
                        material
                            .occlusion_texture()
                            .map(|t| t.strength())
                            .unwrap_or(1.),
                    ],
                    coat: [
                        coat,
                        coat_rough,
                        material
                            .occlusion_texture()
                            .map(|t| t.tex_coord() as f32)
                            .unwrap_or(0.),
                        0.,
                    ],
                },
                textures,
                transparent: material.alpha_mode() == gltf::material::AlphaMode::Blend,
            });
        }
        let scene = gltf
            .default_scene()
            .or_else(|| gltf.scenes().next())
            .ok_or("GLB 没有场景。")?;
        for node in scene.nodes() {
            self.node(node, Mat4::IDENTITY, blob, offset, module)?;
        }
        Ok(())
    }
}
pub fn load(plan: &Plan) -> Result<Scene> {
    let started = Instant::now();
    let mut loader = Loader {
        scene: Scene {
            paint_connections: vec![],
            surfaces: vec![],
            meshes: vec![],
            materials: vec![],
            textures: vec![
                Texture {
                    pixels: vec![255; 4],
                    width: 1,
                    height: 1,
                    srgb: true,
                },
                Texture {
                    pixels: vec![128, 128, 255, 255],
                    width: 1,
                    height: 1,
                    srgb: false,
                },
                Texture {
                    pixels: vec![255; 4],
                    width: 1,
                    height: 1,
                    srgb: false,
                },
            ],
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
            stats: SceneStats::default(),
        },
        texture_keys: HashMap::new(),
        digest: Sha256::new(),
    };
    for module in &plan.modules {
        loader.module(plan, module)?;
    }
    if loader.scene.meshes.is_empty()
        || !loader.scene.min.is_finite()
        || (loader.scene.max - loader.scene.min).length() < 0.01
    {
        return Err("模型场景为空或尺寸无效。".into());
    }
    loader.scene.stats.primitives = loader.scene.meshes.len();
    if plan.paint_map.is_none() {
        crate::surface::prepare(&mut loader.scene, &plan.surfaces)?;
    }
    if let Some(path) = &plan.paint_map {
        let map: itasha_surface_map::PaintSurfaceMap =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        map.validate()?;
        if map.model_fingerprint
            != format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(package_file(&plan.root, "body.glb")?)
                        .map_err(|e| e.to_string())?
                )
            )
        {
            return Err("漆面展开资产与车身模型指纹不一致。".into());
        }
        let sources: HashMap<_, _> = loader
            .scene
            .meshes
            .iter()
            .map(|m| ((m.source_mesh.as_str(), m.source_primitive), m))
            .collect();
        for t in &map.triangles {
            let source = sources
                .get(&(t.id.source_mesh.as_str(), t.id.primitive))
                .ok_or("展开片引用不存在的源网格。")?;
            let indices = source
                .indices
                .get(t.id.source_triangle as usize * 3..t.id.source_triangle as usize * 3 + 3)
                .ok_or("展开片引用不存在的源三角面。")?;
            let points = [indices[0], indices[1], indices[2]]
                .map(|i| Vec3::from(source.vertices[i as usize].position));
            for (p, b) in t.world.iter().zip(t.source_barycentric) {
                if Vec3::from(*p).distance(points[0] * b[0] + points[1] * b[1] + points[2] * b[2])
                    > 0.0001
                {
                    return Err("展开片到原始车身的重心坐标不一致。".into());
                }
            }
        }
        for spec in &plan.surfaces {
            let projection = [
                [spec.u[0], spec.u[1], spec.u[2], 0.],
                [spec.v[0], spec.v[1], spec.v[2], 0.],
                [spec.normal[0], spec.normal[1], spec.normal[2], 1.],
            ];
            let mut surface = crate::surface::SurfaceGeometry {
                info: itasha_contracts::SurfaceInfo {
                    id: spec.id.clone(),
                    name: spec.name.clone(),
                    profile_version: 2,
                    width_mm: 0.,
                    height_mm: 0.,
                    triangles: 0,
                    projection,
                    depth: 0.,
                },
                mesh_id: spec.mesh_id.clone(),
                projection,
                mask: vec![],
                editable_triangles: vec![],
            };
            // The reviewed paint map is the authority for part membership: it was
            // clipped from classified masks. Re-cutting it with the plan's anchor
            // AABB dropped already-classified faces (e.g. rear-left-fender) and
            // left visible 2D gaps, so no second geometric filter runs here.
            let triangles: Vec<_> = map
                .triangles
                .iter()
                .filter(|t| t.part_id == surface.info.id)
                .cloned()
                .collect();
            if triangles.is_empty() {
                return Err(format!("部件缺少展开片：{}", surface.info.id));
            }
            let width = triangles
                .iter()
                .flat_map(|t| t.paint_uv)
                .map(|p| p[0])
                .fold(0f32, f32::max);
            let height = triangles
                .iter()
                .flat_map(|t| t.paint_uv)
                .map(|p| p[1])
                .fold(0f32, f32::max);
            surface.info.width_mm = f64::from(width);
            surface.info.height_mm = f64::from(height);
            surface.info.profile_version = 2;
            surface.mask = triangles
                .iter()
                .flat_map(|t| {
                    t.paint_uv
                        .into_iter()
                        .flat_map(|p| [p[0] / width, p[1] / height])
                })
                .collect();
            surface.info.triangles = triangles.len();
            surface.editable_triangles = triangles;
            loader.scene.surfaces.push(surface);
        }
        loader.scene.paint_connections = map.connections;
        loader.scene.stats.surface_fingerprint = map.paint_fingerprint;
    }
    loader.scene.stats.textures = loader.scene.textures.len();
    loader.scene.stats.parse_ms = started.elapsed().as_secs_f64() * 1000.;
    loader.scene.stats.fingerprint = format!("{:x}", loader.digest.finalize());
    Ok(loader.scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_glb_bakes_mirrored_hierarchy_and_keeps_source_ids() {
        let root = tempfile::tempdir().unwrap();
        let data: [f32; 18] = [
            0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 1., 0., 0., 1., 0., 0., 1., 0.,
        ];
        let binary = bytemuck::cast_slice(&data);
        let document = serde_json::json!({
            "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
            "nodes":[{"translation":[3,2,1],"scale":[-2,1,1],"children":[1]},{"mesh":0}],
            "buffers":[{"byteLength":binary.len()}],
            "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":36}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,0,1]},{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"}],
            "materials":[{}],"meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":1},"material":0}]}]
        });
        let mut json = serde_json::to_vec(&document).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut bytes = vec![];
        for word in [
            0x46546c67u32,
            2,
            (12 + 8 + json.len() + 8 + binary.len()) as u32,
            json.len() as u32,
            0x4e4f534a,
        ] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0x004e4942u32.to_le_bytes());
        bytes.extend_from_slice(binary);
        std::fs::write(root.path().join("fixture.glb"), bytes).unwrap();
        let scene = load(&Plan {
            paint_map: None,
            surfaces: vec![],
            root: root.path().into(),
            modules: vec!["fixture.glb".into()],
            paint: None,
            interior: None,
            texture_limit: 1024,
        })
        .unwrap();
        assert_eq!(scene.min, Vec3::new(1., 2., 1.));
        assert_eq!(scene.max, Vec3::new(3., 2., 2.));
        assert_eq!(scene.meshes[0].indices, vec![0, 2, 1]);
        assert_eq!(scene.meshes[0].id, "fixture.glb/node/1/primitive/0");
        assert_eq!(scene.meshes[0].vertices[1].position, [1., 2., 1.]);
        assert_eq!(scene.stats.triangles, 1);
    }
    #[test]
    fn package_paths_cannot_escape() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "../secret",
            "/secret",
            "https://example.com/a",
            "C:\\secret",
        ] {
            assert!(package_file(dir.path(), name).is_err());
        }
    }
}
