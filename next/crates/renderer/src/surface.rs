use crate::assets::Scene;
use glam::Vec3;
use itasha_contracts::SurfaceInfo;
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq)]
pub struct SurfacePlan {
    pub id: String,
    pub name: String,
    pub mesh_id: String,
    pub node_name: String,
    pub region_bounds: Option<[[f32; 3]; 2]>,
    pub whole_components: bool,
    pub u: [f32; 3],
    pub v: [f32; 3],
    pub normal: [f32; 3],
}
#[derive(Clone)]
pub struct SurfaceGeometry {
    pub info: SurfaceInfo,
    pub mesh_id: String,
    pub projection: [[f32; 4]; 3],
    /// Packed UV triangles, streamed once as binary to the 2D view.
    pub mask: Vec<f32>,
    pub editable_triangles: Vec<itasha_surface_map::PaintTriangle>,
}
#[derive(Clone, Copy)]
struct OccluderTriangle {
    points: [Vec3; 3],
    min: Vec3,
    max: Vec3,
    center: Vec3,
}
#[derive(Clone, Copy)]
struct OccluderNode {
    min: Vec3,
    max: Vec3,
    children: Option<[usize; 2]>,
    start: usize,
    end: usize,
}
struct ExteriorIndex {
    triangles: Vec<OccluderTriangle>,
    nodes: Vec<OccluderNode>,
    mesh_indices: std::collections::HashMap<String, usize>,
}
impl ExteriorIndex {
    fn new(meshes: &[crate::assets::Mesh]) -> Self {
        let mut triangles = Vec::new();
        let mut mesh_indices = std::collections::HashMap::new();
        for (mesh_index, mesh) in meshes.iter().enumerate() {
            mesh_indices.insert(mesh.id.clone(), mesh_index);
            for face in mesh.indices.chunks_exact(3) {
                let points = [face[0], face[1], face[2]]
                    .map(|i| Vec3::from(mesh.vertices[i as usize].position));
                triangles.push(OccluderTriangle {
                    points,
                    min: points[0].min(points[1]).min(points[2]),
                    max: points[0].max(points[1]).max(points[2]),
                    center: (points[0] + points[1] + points[2]) / 3.,
                });
            }
        }
        let mut index = Self {
            triangles,
            nodes: Vec::new(),
            mesh_indices,
        };
        if !index.triangles.is_empty() {
            let len = index.triangles.len();
            index.build(0, len);
        }
        index
    }
    fn build(&mut self, start: usize, end: usize) -> usize {
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for t in &self.triangles[start..end] {
            min = min.min(t.min);
            max = max.max(t.max);
        }
        let index = self.nodes.len();
        self.nodes.push(OccluderNode {
            min,
            max,
            children: None,
            start,
            end,
        });
        if end - start > 8 {
            let extent = max - min;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            self.triangles[start..end]
                .sort_unstable_by(|a, b| a.center[axis].total_cmp(&b.center[axis]));
            let mid = start + (end - start) / 2;
            let left = self.build(start, mid);
            let right = self.build(mid, end);
            self.nodes[index].children = Some([left, right]);
        }
        index
    }
    fn ray_bounds(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> bool {
        let (mut near, mut far) = (0_f32, f32::INFINITY);
        for axis in 0..3 {
            if dir[axis].abs() < 1e-7 {
                if origin[axis] < min[axis] || origin[axis] > max[axis] {
                    return false;
                }
            } else {
                let a = (min[axis] - origin[axis]) / dir[axis];
                let b = (max[axis] - origin[axis]) / dir[axis];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
                if far < near {
                    return false;
                }
            }
        }
        far > 0.
    }
    fn ray_triangle(origin: Vec3, dir: Vec3, p: [Vec3; 3]) -> Option<f32> {
        let e1 = p[1] - p[0];
        let e2 = p[2] - p[0];
        let h = dir.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1e-8 {
            return None;
        }
        let inv = det.recip();
        let s = origin - p[0];
        let u = inv * s.dot(h);
        if !(0. ..=1.).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = inv * dir.dot(q);
        if v < 0. || u + v > 1. {
            return None;
        }
        let t = inv * e2.dot(q);
        (t > 0.).then_some(t)
    }
    fn is_visible(&self, point: Vec3, normal: Vec3, _source: usize) -> bool {
        if self.nodes.is_empty() {
            return true;
        }
        let dir = normal.normalize_or_zero();
        let origin = point + dir * 0.001;
        let mut stack = vec![0];
        while let Some(i) = stack.pop() {
            let node = self.nodes[i];
            if !Self::ray_bounds(origin, dir, node.min, node.max) {
                continue;
            }
            if let Some(children) = node.children {
                stack.extend(children);
                continue;
            }
            for tri in &self.triangles[node.start..node.end] {
                if Self::ray_triangle(origin, dir, tri.points).is_some_and(|t| t > 0.005) {
                    return false;
                }
            }
        }
        true
    }

    /// Exterior paint remains editable if it is visible from any outward view,
    /// rather than only from the triangle's normal (an accessory silhouette).
    fn is_exposed(&self, point: Vec3, normal: Vec3, source: usize) -> bool {
        if self.is_visible(point, normal, source) {
            return true;
        }
        let tangent = normal.any_orthonormal_vector();
        let bitangent = normal.cross(tangent);
        (0..12).any(|i| {
            let angle = i as f32 * std::f32::consts::TAU / 12.;
            self.is_visible(
                point,
                normal + 8. * (tangent * angle.cos() + bitangent * angle.sin()),
                source,
            )
        })
    }

    fn exposed_fragments(
        &self,
        points: [Vec3; 3],
        normal: Vec3,
        source: usize,
        depth: u8,
        output: &mut Vec<[Vec3; 3]>,
    ) {
        let center = (points[0] + points[1] + points[2]) / 3.;
        let visible = points.map(|p| self.is_exposed(p, normal, source));
        let center_visible = self.is_exposed(center, normal, source);
        let edges = [
            points[0].distance_squared(points[1]),
            points[1].distance_squared(points[2]),
            points[2].distance_squared(points[0]),
        ];
        if depth < 9
            && edges.into_iter().fold(0_f32, f32::max) > 0.004_f32.powi(2)
            && (visible.iter().any(|v| *v != center_visible)
                || visible[0] != visible[1]
                || visible[1] != visible[2])
        {
            let mid = [
                (points[0] + points[1]) * 0.5,
                (points[1] + points[2]) * 0.5,
                (points[2] + points[0]) * 0.5,
            ];
            for triangle in [
                [points[0], mid[0], mid[2]],
                [mid[0], points[1], mid[1]],
                [mid[2], mid[1], points[2]],
                [mid[0], mid[1], mid[2]],
            ] {
                self.exposed_fragments(triangle, normal, source, depth + 1, output);
            }
        } else if center_visible {
            output.push(points);
        }
    }
}

fn interpolate_vertex(
    a: crate::assets::Vertex,
    b: crate::assets::Vertex,
    t: f32,
) -> crate::assets::Vertex {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let mut vertex = crate::assets::Vertex {
        position: std::array::from_fn(|i| lerp(a.position[i], b.position[i])),
        normal: std::array::from_fn(|i| lerp(a.normal[i], b.normal[i])),
        uv: std::array::from_fn(|i| lerp(a.uv[i], b.uv[i])),
        uv1: std::array::from_fn(|i| lerp(a.uv1[i], b.uv1[i])),
    };
    let normal = Vec3::from(vertex.normal).normalize_or_zero();
    vertex.normal = normal.to_array();
    vertex
}

fn clip_polygon(
    polygon: &[crate::assets::Vertex],
    axis: usize,
    boundary: f32,
    keep_greater: bool,
) -> Vec<crate::assets::Vertex> {
    let inside = |v: crate::assets::Vertex| {
        if keep_greater {
            v.position[axis] >= boundary
        } else {
            v.position[axis] <= boundary
        }
    };
    let Some(mut previous) = polygon.last().copied() else {
        return Vec::new();
    };
    let mut previous_inside = inside(previous);
    let mut output = Vec::with_capacity(polygon.len() + 1);
    for current in polygon.iter().copied() {
        let current_inside = inside(current);
        if current_inside != previous_inside {
            let t = ((boundary - previous.position[axis])
                / (current.position[axis] - previous.position[axis]))
                .clamp(0.0, 1.0);
            output.push(interpolate_vertex(previous, current, t));
        }
        if current_inside {
            output.push(current);
        }
        previous = current;
        previous_inside = current_inside;
    }
    output
}

fn triangulate_polygon(polygon: &[crate::assets::Vertex]) -> Vec<[crate::assets::Vertex; 3]> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    (1..polygon.len() - 1)
        .map(|i| [polygon[0], polygon[i], polygon[i + 1]])
        .filter(|triangle| {
            let a = Vec3::from(triangle[0].position);
            let b = Vec3::from(triangle[1].position);
            let c = Vec3::from(triangle[2].position);
            (b - a).cross(c - a).length_squared() > 1e-16
        })
        .collect()
}

// Partition a triangle into the portion inside an AABB and disjoint outside fragments.
// Clipping, rather than centroid assignment, prevents long boundary-crossing triangles
// from leaking into neighboring paint surfaces and their 2D masks.
fn clip_triangle_to_bounds(
    triangle: [crate::assets::Vertex; 3],
    bounds: [[f32; 3]; 2],
) -> (
    Vec<[crate::assets::Vertex; 3]>,
    Vec<[crate::assets::Vertex; 3]>,
) {
    let mut inside = vec![triangle.to_vec()];
    let mut outside = Vec::new();
    for (axis, _) in bounds[0].iter().enumerate() {
        for (boundary, keep_greater) in [(bounds[0][axis], true), (bounds[1][axis], false)] {
            let mut next_inside = Vec::new();
            for polygon in inside {
                let clipped_inside = clip_polygon(&polygon, axis, boundary, keep_greater);
                let clipped_outside = clip_polygon(&polygon, axis, boundary, !keep_greater);
                outside.extend(triangulate_polygon(&clipped_outside));
                if clipped_inside.len() >= 3 {
                    next_inside.push(clipped_inside);
                }
            }
            inside = next_inside;
        }
    }
    (
        inside.iter().flat_map(|p| triangulate_polygon(p)).collect(),
        outside.into_iter().collect(),
    )
}

struct SourceIdentity<'a> {
    mesh: &'a str,
    primitive: u32,
    faces: &'a [crate::assets::SourceFaceId],
}

fn compact_mesh(
    id: String,
    name: String,
    material: usize,
    center: Vec3,
    triangles: Vec<(usize, [crate::assets::Vertex; 3])>,
    source: SourceIdentity<'_>,
) -> crate::assets::Mesh {
    let mut vertices = Vec::with_capacity(triangles.len() * 3);
    let mut indices = Vec::with_capacity(triangles.len() * 3);
    let mut fragments = std::collections::HashMap::<u32, u16>::new();
    let mut output_faces = Vec::with_capacity(triangles.len());
    for (source_index, triangle) in triangles {
        let mut source_face = source.faces[source_index];
        let fragment = fragments.entry(source_face.triangle).or_default();
        source_face.fragment = *fragment;
        *fragment = fragment.saturating_add(1);
        output_faces.push(source_face);
        for vertex in triangle {
            indices.push(vertices.len() as u32);
            vertices.push(vertex);
        }
    }
    crate::assets::Mesh {
        id,
        source_mesh: source.mesh.to_string(),
        name,
        vertices,
        indices,
        source_primitive: source.primitive,
        source_faces: output_faces,
        material,
        center,
    }
}

fn mesh_face_components(mesh: &crate::assets::Mesh) -> Vec<usize> {
    let mut parents: Vec<usize> = (0..mesh.vertices.len()).collect();
    fn root(parents: &mut [usize], mut index: usize) -> usize {
        while parents[index] != index {
            parents[index] = parents[parents[index]];
            index = parents[index];
        }
        index
    }
    for face in mesh.indices.chunks_exact(3) {
        let first = root(&mut parents, face[0] as usize);
        for index in &face[1..] {
            let other = root(&mut parents, *index as usize);
            parents[other] = first;
        }
    }
    mesh.indices
        .chunks_exact(3)
        .map(|face| root(&mut parents, face[0] as usize))
        .collect()
}

pub fn prepare(scene: &mut Scene, plans: &[SurfacePlan]) -> Result<(), String> {
    let exterior_index = ExteriorIndex::new(&scene.meshes);
    let source_geometry: std::collections::HashMap<_, Vec<[Vec3; 3]>> = scene
        .meshes
        .iter()
        .map(|mesh| {
            (
                mesh.id.clone(),
                mesh.indices
                    .chunks_exact(3)
                    .map(|t| {
                        t.try_into()
                            .map(|t: [u32; 3]| {
                                t.map(|i| Vec3::from(mesh.vertices[i as usize].position))
                            })
                            .unwrap()
                    })
                    .collect(),
            )
        })
        .collect();
    let mut digest = Sha256::new();
    digest.update(b"itasha-paint-surface-map-v1");
    // Ordered AABBs clip source triangles so surfaces cannot inherit geometry across
    // an artificial region boundary.
    let mut remaining =
        std::collections::HashMap::<String, Vec<(usize, [crate::assets::Vertex; 3])>>::new();
    for plan in plans.iter().filter(|p| p.region_bounds.is_some()) {
        if !remaining.contains_key(&plan.mesh_id) {
            let source = scene
                .meshes
                .iter()
                .find(|m| m.id == plan.mesh_id)
                .ok_or("区域来源网格缺失")?;
            let triangles = source
                .indices
                .chunks_exact(3)
                .enumerate()
                .map(|(index, face)| {
                    (
                        index,
                        [face[0], face[1], face[2]].map(|i| source.vertices[i as usize]),
                    )
                })
                .collect();
            remaining.insert(plan.mesh_id.clone(), triangles);
        }
        let bounds = plan.region_bounds.unwrap();
        let mut selected = Vec::new();
        let mut rest = Vec::new();
        let triangles = remaining.remove(&plan.mesh_id).unwrap_or_default();
        if plan.whole_components {
            let source = scene
                .meshes
                .iter()
                .find(|m| m.id == plan.mesh_id)
                .ok_or("区域来源网格缺失")?;
            let face_components = mesh_face_components(source);
            let mut component_bounds = std::collections::HashMap::<usize, [[f32; 3]; 2]>::new();
            for (face_index, face) in source.indices.chunks_exact(3).enumerate() {
                let component = face_components[face_index];
                let component_bounds = component_bounds
                    .entry(component)
                    .or_insert([[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]]);
                for index in face {
                    let position = source.vertices[*index as usize].position;
                    for axis in 0..3 {
                        component_bounds[0][axis] = component_bounds[0][axis].min(position[axis]);
                        component_bounds[1][axis] = component_bounds[1][axis].max(position[axis]);
                    }
                }
            }
            let components: std::collections::HashSet<_> = component_bounds
                .into_iter()
                .filter_map(|(component, component_bounds)| {
                    (0..3)
                        .all(|axis| {
                            (component_bounds[0][axis] + component_bounds[1][axis]) * 0.5
                                >= bounds[0][axis]
                                && (component_bounds[0][axis] + component_bounds[1][axis]) * 0.5
                                    <= bounds[1][axis]
                        })
                        .then_some(component)
                })
                .collect();
            for (face_index, triangle) in triangles {
                if components.contains(&face_components[face_index]) {
                    selected.push((face_index, triangle));
                } else {
                    rest.push((face_index, triangle));
                }
            }
        } else {
            for (face_index, triangle) in triangles {
                let (clipped, remainder) = clip_triangle_to_bounds(triangle, bounds);
                selected.extend(clipped.into_iter().map(|triangle| (face_index, triangle)));
                rest.extend(remainder.into_iter().map(|triangle| (face_index, triangle)));
            }
        }
        remaining.insert(plan.mesh_id.clone(), rest);
        let source = scene
            .meshes
            .iter()
            .find(|m| m.id == plan.mesh_id)
            .ok_or("区域来源网格缺失")?;
        scene.meshes.push(compact_mesh(
            format!("{}/region/{}", plan.mesh_id, plan.id),
            source.name.clone(),
            source.material,
            source.center,
            selected,
            SourceIdentity {
                mesh: &source.source_mesh,
                primitive: source.source_primitive,
                faces: &source.source_faces,
            },
        ));
    }
    for (id, triangles) in remaining {
        let source = scene.meshes.iter_mut().find(|m| m.id == id).unwrap();
        *source = compact_mesh(
            id,
            source.name.clone(),
            source.material,
            source.center,
            triangles,
            SourceIdentity {
                mesh: &source.source_mesh,
                primitive: source.source_primitive,
                faces: &source.source_faces,
            },
        );
    }
    for plan in plans {
        let mesh_id = if plan.region_bounds.is_some() {
            format!("{}/region/{}", plan.mesh_id, plan.id)
        } else {
            plan.mesh_id.clone()
        };
        let mesh = scene
            .meshes
            .iter()
            .find(|m| m.id == mesh_id)
            .ok_or("外板节点缺失，禁止猜测部件映射。")?;
        if mesh.name != plan.node_name || !scene.materials[mesh.material].paint {
            return Err("外板来源名称/车漆材质与适配器不一致。".into());
        }
        let u = Vec3::from(plan.u);
        let v = Vec3::from(plan.v);
        let normal = Vec3::from(plan.normal);
        if (u.length() - 1.).abs() > 1e-5
            || (v.length() - 1.).abs() > 1e-5
            || (normal.length() - 1.).abs() > 1e-5
            || u.dot(v).abs() > 1e-5
            || u.dot(normal).abs() > 1e-5
            || v.dot(normal).abs() > 1e-5
        {
            return Err("部件投影基不正交。".into());
        }
        let mut triangles = vec![];
        let mut editable_triangles = vec![];
        let mut exterior_depths = Vec::new();
        let vehicle_center = (scene.min + scene.max) * 0.5;
        for (face_index, face) in mesh.indices.chunks_exact(3).enumerate() {
            let points = [face[0], face[1], face[2]]
                .map(|index| Vec3::from(mesh.vertices[index as usize].position));
            let face_normal = (points[1] - points[0])
                .cross(points[2] - points[0])
                .normalize_or_zero();
            let centroid = (points[0] + points[1] + points[2]) / 3.;
            if face_normal.dot(centroid - vehicle_center) <= 0.01 {
                continue;
            }
            let source = *exterior_index
                .mesh_indices
                .get(&plan.mesh_id)
                .ok_or("外板来源网格缺失")?;
            let mut fragments = Vec::new();
            exterior_index.exposed_fragments(points, face_normal, source, 0, &mut fragments);
            for points in fragments {
                let source_face = mesh.source_faces[face_index];
                let original = source_geometry[&mesh.source_mesh][source_face.triangle as usize];
                let mut hash = Sha256::new();
                hash.update(bytemuck::cast_slice(&points.map(|p| p.to_array())));
                let fragment = u64::from_le_bytes(hash.finalize()[..8].try_into().unwrap());
                editable_triangles.push(itasha_surface_map::PaintTriangle {
                    id: itasha_surface_map::TriangleId {
                        source_mesh: mesh.source_mesh.clone(),
                        primitive: mesh.source_primitive,
                        source_triangle: source_face.triangle,
                        fragment,
                    },
                    part_id: plan.id.clone(),
                    region_id: format!("{}/exterior", plan.id),
                    chart_id: plan.id.clone(),
                    world: points.map(|p| p.to_array()),
                    source_barycentric: points.map(|p| barycentric_world(p, original).to_array()),
                    paint_uv: points.map(|p| [p.dot(u), p.dot(v)]),
                });
                let centroid = (points[0] + points[1] + points[2]) / 3.;
                exterior_depths.push(centroid.dot(normal));
                triangles.push(points.map(|p| [p.dot(u), p.dot(v)]));
            }
        }
        if triangles.is_empty() {
            // Some source paint nodes only contain hidden or inward-facing geometry.
            // Keep the vehicle usable and omit those regions from the editable surface list.
            continue;
        }
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for p in triangles.iter().flatten() {
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let size = [max[0] - min[0], max[1] - min[1]];
        if size.iter().any(|v| *v < 0.001) {
            return Err("外板投影尺寸无效。".into());
        }
        let mask = triangles
            .iter()
            .flat_map(|t| {
                t.iter()
                    .flat_map(|p| [(p[0] - min[0]) / size[0], (p[1] - min[1]) / size[1]])
            })
            .collect();
        for triangle in &mut editable_triangles {
            triangle.paint_uv = triangle
                .paint_uv
                .map(|p| [(p[0] - min[0]) * 1000., (p[1] - min[1]) * 1000.]);
        }
        digest.update(plan.id.as_bytes());
        digest.update(plan.mesh_id.as_bytes());
        digest.update(bytemuck::cast_slice(&[plan.u, plan.v, plan.normal]));
        digest.update(bytemuck::cast_slice(&mesh.vertices));
        digest.update(bytemuck::cast_slice(&mesh.indices));
        // Visibility changes may alter the millimetre origin and dimensions even
        // when source mesh bytes are identical. Never silently reuse old placements.
        digest.update(bytemuck::cast_slice(&[min, max]));
        let projection = [
            [
                u.x / size[0],
                u.y / size[0],
                u.z / size[0],
                -min[0] / size[0],
            ],
            [
                v.x / size[1],
                v.y / size[1],
                v.z / size[1],
                -min[1] / size[1],
            ],
            [normal.x, normal.y, normal.z, 1.],
        ];
        scene.surfaces.push(SurfaceGeometry {
            info: SurfaceInfo {
                id: plan.id.clone(),
                name: plan.name.clone(),
                profile_version: 1,
                width_mm: f64::from(size[0]) * 1000.,
                height_mm: f64::from(size[1]) * 1000.,
                triangles: triangles.len(),
                projection,
                depth: exterior_depths.iter().sum::<f32>() / exterior_depths.len().max(1) as f32,
            },
            mesh_id: mesh.id.clone(),
            projection: [
                [
                    u.x / size[0],
                    u.y / size[0],
                    u.z / size[0],
                    -min[0] / size[0],
                ],
                [
                    v.x / size[1],
                    v.y / size[1],
                    v.z / size[1],
                    -min[1] / size[1],
                ],
                [normal.x, normal.y, normal.z, 1.],
            ],
            mask,
            editable_triangles,
        });
    }
    scene.stats.surface_fingerprint = format!("{:x}", digest.finalize());
    Ok(())
}

fn barycentric_world(p: Vec3, t: [Vec3; 3]) -> Vec3 {
    let a = t[1] - t[0];
    let b = t[2] - t[0];
    let q = p - t[0];
    let aa = a.dot(a);
    let ab = a.dot(b);
    let bb = b.dot(b);
    let aq = a.dot(q);
    let bq = b.dot(q);
    let det = aa * bb - ab * ab;
    if det.abs() < 1e-20 {
        return Vec3::X;
    }
    let y = (bb * aq - ab * bq) / det;
    let z = (aa * bq - ab * aq) / det;
    Vec3::new(1. - y - z, y, z)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Material, MaterialUniform, Mesh, SceneStats, Vertex};
    use bytemuck::Zeroable;

    fn occluding_plate(z: f32, radius: f32) -> Mesh {
        Mesh {
            id: "accessory".into(),
            source_mesh: "accessory".into(),
            name: "accessory".into(),
            vertices: [
                [-radius, -radius, z],
                [radius, -radius, z],
                [radius, radius, z],
                [-radius, radius, z],
            ]
            .map(|position| Vertex {
                position,
                normal: [0., 0., 1.],
                uv: [0.; 2],
                uv1: [0.; 2],
            })
            .to_vec(),
            indices: vec![0, 1, 2, 0, 2, 3],
            source_primitive: 0,
            source_faces: vec![
                crate::assets::SourceFaceId {
                    triangle: 0,
                    fragment: 0,
                },
                crate::assets::SourceFaceId {
                    triangle: 1,
                    fragment: 0,
                },
            ],
            material: 0,
            center: Vec3::new(0., 0., z),
        }
    }

    #[test]
    fn exterior_visibility_keeps_paint_below_overhang_but_excludes_attachment_footprint() {
        let overhang = ExteriorIndex::new(&[occluding_plate(0.15, 0.1)]);
        assert!(!overhang.is_visible(Vec3::ZERO, Vec3::Z, usize::MAX));
        assert!(overhang.is_exposed(Vec3::ZERO, Vec3::Z, usize::MAX));
        let attachment = ExteriorIndex::new(&[occluding_plate(0.02, 0.5)]);
        assert!(!attachment.is_exposed(Vec3::ZERO, Vec3::Z, usize::MAX));
        assert!(attachment.is_exposed(Vec3::X, Vec3::Z, usize::MAX));
    }

    #[test]
    fn partial_occlusion_refines_triangle_instead_of_deleting_entire_face() {
        let index = ExteriorIndex::new(&[occluding_plate(0.02, 0.5)]);
        let triangle = [
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(0., 1., 0.),
        ];
        let mut fragments = vec![];
        index.exposed_fragments(triangle, Vec3::Z, usize::MAX, 0, &mut fragments);
        assert!(fragments.len() > 1);
        let area = fragments
            .iter()
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5)
            .sum::<f32>();
        assert!(area > 0.5 && area < 2.);
        assert!(fragments.iter().all(|t| index.is_exposed(
            (t[0] + t[1] + t[2]) / 3.,
            Vec3::Z,
            usize::MAX
        )));
    }

    #[test]
    fn ordered_regions_clip_boundary_crossing_triangles_without_area_loss_or_degenerates() {
        let positions = [
            [-2., 0., 1.],
            [-1., 0., 1.],
            [-1., 1., 1.],
            [1., 0., 1.],
            [2., 0., 1.],
            [2., 1., 1.],
        ];
        let mut scene = Scene {
            paint_connections: vec![],
            surfaces: vec![],
            meshes: vec![Mesh {
                id: "combined".into(),
                source_mesh: "combined".into(),
                name: "paint-shell".into(),
                vertices: positions
                    .into_iter()
                    .map(|position| Vertex {
                        position,
                        normal: [0., 0., 1.],
                        uv: [0.; 2],
                        uv1: [0.; 2],
                    })
                    .collect(),
                indices: vec![0, 1, 2, 3, 4, 5],
                source_primitive: 0,
                source_faces: vec![
                    crate::assets::SourceFaceId {
                        triangle: 0,
                        fragment: 0,
                    },
                    crate::assets::SourceFaceId {
                        triangle: 1,
                        fragment: 0,
                    },
                ],
                material: 0,
                center: Vec3::ZERO,
            }],
            materials: vec![Material {
                paint: true,
                uniform: MaterialUniform::zeroed(),
                textures: [0; 4],
                transparent: false,
            }],
            textures: vec![],
            min: Vec3::splat(-2.),
            max: Vec3::splat(2.),
            stats: SceneStats::default(),
        };
        let plans = [
            ("left", [[-3., -3., -3.], [0., 3., 3.]]),
            ("right", [[-3., -3., -3.], [3., 3., 3.]]),
        ]
        .map(|(id, bounds)| SurfacePlan {
            id: id.into(),
            name: id.into(),
            mesh_id: "combined".into(),
            node_name: "paint-shell".into(),
            region_bounds: Some(bounds),
            whole_components: false,
            u: [1., 0., 0.],
            v: [0., -1., 0.],
            normal: [0., 0., 1.],
        });
        prepare(&mut scene, &plans).unwrap();
        assert!(scene.meshes[0].indices.is_empty());
        let area = |mesh: &Mesh| {
            mesh.indices
                .chunks_exact(3)
                .map(|face| {
                    let [a, b, c] = [face[0], face[1], face[2]]
                        .map(|i| Vec3::from(mesh.vertices[i as usize].position));
                    (b - a).cross(c - a).length() * 0.5
                })
                .sum::<f32>()
        };
        assert!((area(&scene.meshes[1]) - 0.5).abs() < 1e-5);
        assert!((area(&scene.meshes[2]) - 0.5).abs() < 1e-5);
        for mesh in &scene.meshes {
            for face in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [face[0], face[1], face[2]]
                    .map(|i| Vec3::from(mesh.vertices[i as usize].position));
                assert!((b - a).cross(c - a).length_squared() > 1e-16);
            }
        }
        assert_eq!(scene.surfaces.len(), 2);
        assert!(scene.surfaces.iter().all(|s| s.info.triangles > 0));
    }

    #[test]
    fn component_region_extracts_only_the_disconnected_accessory_mesh() {
        let positions = [
            [-1.2, 0.3, 0.94],
            [-1.1, 0.3, 0.94],
            [-1.1, 0.4, 0.94],
            [0.5, 0.3, 0.94],
            [0.6, 0.3, 0.94],
            [0.6, 0.4, 0.94],
        ];
        let mut scene = Scene {
            paint_connections: vec![],
            surfaces: vec![],
            meshes: vec![Mesh {
                id: "combined".into(),
                source_mesh: "combined".into(),
                name: "paint-shell".into(),
                vertices: positions
                    .into_iter()
                    .map(|position| Vertex {
                        position,
                        normal: [0., 0., 1.],
                        uv: [0.; 2],
                        uv1: [0.; 2],
                    })
                    .collect(),
                indices: vec![0, 1, 2, 3, 4, 5],
                source_primitive: 0,
                source_faces: vec![
                    crate::assets::SourceFaceId {
                        triangle: 0,
                        fragment: 0,
                    },
                    crate::assets::SourceFaceId {
                        triangle: 1,
                        fragment: 0,
                    },
                ],
                material: 0,
                center: Vec3::ZERO,
            }],
            materials: vec![Material {
                paint: true,
                uniform: MaterialUniform::zeroed(),
                textures: [0; 4],
                transparent: false,
            }],
            textures: vec![],
            min: Vec3::splat(-2.),
            max: Vec3::splat(2.),
            stats: SceneStats::default(),
        };
        let plans = [SurfacePlan {
            id: "lower-fender".into(),
            name: "下翼子板".into(),
            mesh_id: "combined".into(),
            node_name: "paint-shell".into(),
            // The anchor contains the component center, not its full extent.
            region_bounds: Some([[-1.16, 0.34, 0.93], [-1.14, 0.36, 0.95]]),
            whole_components: true,
            u: [1., 0., 0.],
            v: [0., -1., 0.],
            normal: [0., 0., 1.],
        }];

        prepare(&mut scene, &plans).unwrap();

        let accessory = scene
            .meshes
            .iter()
            .find(|mesh| mesh.id.ends_with("/lower-fender"))
            .unwrap();
        assert_eq!(accessory.indices.len(), 3);
        assert_eq!(scene.meshes[0].indices.len(), 3);
        assert_eq!(scene.surfaces[0].info.triangles, 1);
    }
    #[test]
    fn curved_part_projection_keeps_real_geometry_excludes_backs_and_is_material_independent() {
        let mut vertices = vec![];
        for angle in [-0.7_f32, 0., 0.7] {
            for y in [-0.5, 0.5] {
                vertices.push(Vertex {
                    position: [angle.sin(), y, angle.cos()],
                    normal: [angle.sin(), 0., angle.cos()],
                    uv: [0.; 2],
                    uv1: [0.; 2],
                })
            }
        }
        let indices = vec![0, 2, 1, 1, 2, 3, 2, 4, 3, 3, 4, 5, 1, 2, 0];
        let before: Vec<_> = vertices.iter().map(|v| v.position).collect();
        let mut scene = Scene {
            paint_connections: vec![],
            surfaces: vec![],
            meshes: vec![Mesh {
                id: "fixture".into(),
                source_mesh: "fixture".into(),
                name: "outer".into(),
                vertices,
                indices,
                source_primitive: 0,
                source_faces: (0..5)
                    .map(|triangle| crate::assets::SourceFaceId {
                        triangle,
                        fragment: 0,
                    })
                    .collect(),
                material: 0,
                center: Vec3::ZERO,
            }],
            materials: vec![Material {
                paint: true,
                uniform: MaterialUniform::zeroed(),
                textures: [0; 4],
                transparent: false,
            }],
            textures: vec![],
            min: Vec3::splat(-1.),
            max: Vec3::splat(1.),
            stats: SceneStats::default(),
        };
        let plan = SurfacePlan {
            id: "door".into(),
            name: "测试曲面".into(),
            mesh_id: "fixture".into(),
            node_name: "outer".into(),
            region_bounds: None,
            whole_components: false,
            u: [1., 0., 0.],
            v: [0., -1., 0.],
            normal: [0., 0., 1.],
        };
        prepare(&mut scene, std::slice::from_ref(&plan)).unwrap();
        assert_eq!(scene.surfaces[0].info.triangles, 4);
        assert!(
            scene.surfaces[0]
                .mask
                .iter()
                .all(|v| (-1e-6..=1.000001).contains(v))
        );
        assert_eq!(
            scene.meshes[0]
                .vertices
                .iter()
                .map(|v| v.position)
                .collect::<Vec<_>>(),
            before
        );
        let hash = scene.stats.surface_fingerprint.clone();
        scene.materials[0].uniform.color = [1.; 4];
        scene.surfaces.clear();
        prepare(&mut scene, std::slice::from_ref(&plan)).unwrap();
        assert_eq!(hash, scene.stats.surface_fingerprint);
        scene.surfaces.clear();
        scene.materials[0].paint = false;
        assert!(prepare(&mut scene, &[plan]).is_err());
    }
}
