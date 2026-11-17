//! CPU geometry queries shared by native selection, drag and control points.
use crate::{assets::Scene, surface::SurfaceGeometry};
use glam::{Vec2, Vec3};
use itasha_contracts::{Layer, SurfaceInfo};
#[derive(Clone)]
struct Triangle {
    p: [Vec3; 3],
    surface: Option<usize>,
    id: itasha_surface_map::TriangleId,
}
struct Node {
    min: Vec3,
    max: Vec3,
    children: Option<(Box<Node>, Box<Node>)>,
    faces: Vec<Triangle>,
}
impl Node {
    fn build(mut faces: Vec<Triangle>) -> Self {
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for p in faces.iter().flat_map(|t| t.p) {
            min = min.min(p);
            max = max.max(p);
        }
        let children = if faces.len() > 16 {
            let extent = max - min;
            let axis = if extent.x > extent.y && extent.x > extent.z {
                0
            } else if extent.y > extent.z {
                1
            } else {
                2
            };
            let mid = faces.len() / 2;
            faces.select_nth_unstable_by(mid, |a, b| {
                a.p.iter()
                    .map(|p| p[axis])
                    .sum::<f32>()
                    .total_cmp(&b.p.iter().map(|p| p[axis]).sum::<f32>())
            });
            let right = faces.split_off(faces.len() / 2);
            let left = std::mem::take(&mut faces);
            Some((Box::new(Self::build(left)), Box::new(Self::build(right))))
        } else {
            None
        };
        Self {
            min,
            max,
            children,
            faces,
        }
    }
    fn hit(
        &self,
        origin: Vec3,
        direction: Vec3,
        best: &mut Option<(
            f32,
            Option<usize>,
            Vec3,
            itasha_surface_map::TriangleId,
            Vec3,
        )>,
    ) {
        let (mut near, mut far) = (0_f32, best.as_ref().map(|v| v.0).unwrap_or(f32::INFINITY));
        for i in 0..3 {
            if direction[i].abs() < 1e-8 {
                if origin[i] < self.min[i] || origin[i] > self.max[i] {
                    return;
                }
            } else {
                let a = (self.min[i] - origin[i]) / direction[i];
                let b = (self.max[i] - origin[i]) / direction[i];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
        }
        if far < near {
            return;
        }
        if let Some((a, b)) = &self.children {
            a.hit(origin, direction, best);
            b.hit(origin, direction, best);
            return;
        }
        for face in &self.faces {
            let p = face.p.map(|p| p.as_dvec3());
            let origin = origin.as_dvec3();
            let direction = direction.as_dvec3();
            let e1 = p[1] - p[0];
            let e2 = p[2] - p[0];
            let normal = e1.cross(e2);
            let h = direction.cross(e2);
            let det = e1.dot(h);
            // Test the ray's angle, not an area-dependent determinant. A valid
            // microscopic triangle must remain selectable along its normal.
            if !det.is_finite() || det.abs() <= normal.length() * direction.length() * 1e-12 {
                continue;
            }
            let inv = 1. / det;
            let s = origin - p[0];
            let u = inv * s.dot(h);
            if !(0. ..=1.).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = inv * direction.dot(q);
            if v < 0. || u + v > 1. {
                continue;
            }
            let t = inv * e2.dot(q);
            if t > 0.00001 && best.as_ref().is_none_or(|b| t < f64::from(b.0)) {
                *best = Some((
                    t as f32,
                    face.surface,
                    normal.normalize().as_vec3(),
                    face.id.clone(),
                    Vec3::new((1. - u - v) as f32, u as f32, v as f32),
                ));
            }
        }
    }
}
pub struct Geometry {
    pub topology: itasha_surface_map::topology::Topology,
    pub surfaces: Vec<SurfaceGeometry>,
    root: Node,
    paint_faces: std::collections::HashMap<(String, u32), Vec<(usize, usize)>>,
}
#[derive(Clone)]
pub struct Hit {
    pub surface: usize,
    pub world: Vec3,
    pub normal: Vec3,
    pub triangle_id: itasha_surface_map::TriangleId,
    pub barycentric: Vec3,
    pub paint_uv: Vec2,
}
#[derive(Clone, Debug)]
pub struct SurfaceEditPoint {
    pub surface_id: String,
    pub point: Vec2,
}
impl Geometry {
    pub fn constrain(&self, previous: &Layer, next: &Layer) -> Layer {
        if self
            .topology
            .locate(
                &next.placement.surface_id,
                Vec2::new(next.placement.x_mm as f32, next.placement.y_mm as f32),
            )
            .is_some()
        {
            next.clone()
        } else {
            previous.clone()
        }
    }
    pub fn new(scene: &Scene) -> Self {
        let mut paint_faces: std::collections::HashMap<_, Vec<_>> =
            std::collections::HashMap::new();
        for (surface_index, surface) in scene.surfaces.iter().enumerate() {
            for (index, t) in surface.editable_triangles.iter().enumerate() {
                paint_faces
                    .entry((t.id.source_mesh.clone(), t.id.source_triangle))
                    .or_default()
                    .push((surface_index, index));
            }
        }
        let faces = scene
            .meshes
            .iter()
            .flat_map(|m| {
                let surface = scene.surfaces.iter().position(|s| s.mesh_id == m.id);
                m.indices
                    .chunks_exact(3)
                    .enumerate()
                    .map(move |(index, t)| Triangle {
                        p: [t[0], t[1], t[2]].map(|i| Vec3::from(m.vertices[i as usize].position)),
                        surface,
                        id: itasha_surface_map::TriangleId {
                            source_mesh: m.source_mesh.clone(),
                            primitive: m.source_primitive,
                            source_triangle: m.source_faces[index].triangle,
                            fragment: u64::from(m.source_faces[index].fragment),
                        },
                    })
            })
            .collect();
        Self {
            topology: itasha_surface_map::topology::Topology::new(
                scene
                    .surfaces
                    .iter()
                    .flat_map(|s| s.editable_triangles.clone())
                    .collect(),
                &scene.paint_connections,
            ),
            surfaces: scene.surfaces.clone(),
            root: Node::build(faces),
            paint_faces,
        }
    }
    pub fn pick(&self, origin: Vec3, direction: Vec3) -> Option<Hit> {
        let mut best = None;
        self.root.hit(origin, direction, &mut best);
        let (t, surface, normal, triangle_id, _barycentric) = best?;
        let world = origin + direction * t;
        if let Some(candidates) = self
            .paint_faces
            .get(&(triangle_id.source_mesh.clone(), triangle_id.source_triangle))
        {
            for (surface, index) in candidates {
                let triangle = &self.surfaces[*surface].editable_triangles[*index];
                let Some(b) = barycentric_world(world, triangle.world.map(Vec3::from_array)) else {
                    continue;
                };
                if b.min_element() < -0.002 {
                    continue;
                }
                let uv = triangle.paint_uv.map(Vec2::from_array);
                return Some(Hit {
                    surface: *surface,
                    world,
                    normal,
                    triangle_id: triangle.id.clone(),
                    barycentric: b,
                    paint_uv: uv[0] * b.x + uv[1] * b.y + uv[2] * b.z,
                });
            }
            return None;
        }
        let _ = (surface, _barycentric);
        None
    }
    pub fn info(&self, id: &str) -> Option<&SurfaceInfo> {
        self.surfaces
            .iter()
            .find(|s| s.info.id == id)
            .map(|s| &s.info)
    }
    pub fn coordinates(&self, layer: &Layer, world: Vec3) -> Option<Vec2> {
        let surface = self
            .surfaces
            .iter()
            .find(|s| s.info.id == layer.placement.surface_id)?;
        for triangle in &surface.editable_triangles {
            let points = triangle.world.map(Vec3::from_array);
            let Some(b) = barycentric_world(world, points) else {
                continue;
            };
            if b.min_element() >= -0.002
                && world.distance(points[0] * b.x + points[1] * b.y + points[2] * b.z) < 0.002
            {
                let uv = triangle.paint_uv.map(Vec2::from_array);
                return Some(uv[0] * b.x + uv[1] * b.y + uv[2] * b.z);
            }
        }
        None
    }
    pub fn world(&self, layer: &Layer, point: Vec2) -> Option<Vec3> {
        let info = self.info(&layer.placement.surface_id)?;
        let surface = self.surfaces.iter().find(|s| s.info.id == info.id)?;
        let mut nearest = None;
        for triangle in &surface.editable_triangles {
            let uv = triangle.paint_uv.map(Vec2::from_array);
            let Some(b) = itasha_surface_map::barycentric_2d(point, uv) else {
                continue;
            };
            let p = triangle.world.map(Vec3::from_array);
            let world = p[0] * b.x + p[1] * b.y + p[2] * b.z;
            if b.min_element() >= -0.0001 {
                return Some(world);
            }
            let distance = point.distance_squared((uv[0] + uv[1] + uv[2]) / 3.);
            if nearest.is_none_or(|(d, _)| distance < d) {
                nearest = Some((distance, world));
            }
        }
        nearest.map(|(_, p)| p)
    }
    pub fn plane_point(&self, layer: &Layer, origin: Vec3, direction: Vec3) -> Option<Vec2> {
        let center = Vec2::new(layer.placement.x_mm as f32, layer.placement.y_mm as f32);
        let (p, ux, uy) = self
            .topology
            .tangent_frame(&layer.placement.surface_id, center)?;
        let normal = ux.cross(uy).normalize_or_zero();
        let den = normal.dot(direction);
        if den.abs() < 0.001 {
            return None;
        }
        let t = normal.dot(p - origin) / den;
        if t <= 0. {
            return None;
        }
        let delta = origin + direction * t - p;
        let a = ux.dot(ux);
        let b = ux.dot(uy);
        let c = uy.dot(uy);
        let det = a * c - b * b;
        if det.abs() < 1e-24 {
            return None;
        }
        self.topology.chart_offset(
            &layer.placement.surface_id,
            center,
            Vec2::new(
                (c * ux.dot(delta) - b * uy.dot(delta)) / det,
                (a * uy.dot(delta) - b * ux.dot(delta)) / det,
            ),
        )
    }

    /// Physical-mm axes shared with coverage. Chart scale/shear must not skew
    /// the controls after the artwork itself has been unfolded isometrically.
    pub fn anchor_frame(&self, layer: &Layer) -> Option<(Vec3, Vec3, Vec3)> {
        let p = &layer.placement;
        let center = Vec2::new(p.x_mm as f32, p.y_mm as f32);
        self.topology.tangent_frame(&p.surface_id, center)
    }

    /// Resolve pointer input against whichever visible paint surface is under it.
    /// A surface change is a placement re-anchor; it does not depend on the
    /// adjacent-surface coverage option.
    pub fn edit_point(
        &self,
        layer: &Layer,
        origin: Vec3,
        direction: Vec3,
        plane: bool,
    ) -> Option<SurfaceEditPoint> {
        if !plane && let Some(hit) = self.pick(origin, direction) {
            return Some(SurfaceEditPoint {
                surface_id: self.surfaces[hit.surface].info.id.clone(),
                point: hit.paint_uv,
            });
        }
        let point = self.plane_point(layer, origin, direction)?;
        Some(SurfaceEditPoint {
            surface_id: layer.placement.surface_id.clone(),
            point,
        })
    }
}

/// Place the same graphic under the current pointer on a paint chart while
/// retaining the clicked point's offset inside the graphic.
pub fn image_grab_offset(layer: &Layer, image_uv: Vec2) -> Vec2 {
    // Coverage UVs precede the optional texture flip. Applying flip_x here
    // again would move the opposite image point under the pointer.
    (image_uv - Vec2::splat(0.5))
        * Vec2::new(
            layer.placement.width_mm as f32,
            layer.placement.height_mm as f32,
        )
}
pub fn move_anchor(layer: &Layer, point: &SurfaceEditPoint, grab_offset: [f32; 2]) -> Layer {
    let mut moved = layer.clone();
    let rotation = glam::Mat2::from_angle((layer.placement.rotation_deg as f32).to_radians());
    let offset = rotation * Vec2::from_array(grab_offset);
    moved.placement.surface_id = point.surface_id.clone();
    moved.placement.x_mm = f64::from(point.point.x - offset.x);
    moved.placement.y_mm = f64::from(point.point.y - offset.y);
    moved
}
pub fn transport_anchor(
    topology: &itasha_surface_map::topology::Topology,
    layer: &Layer,
    point: &SurfaceEditPoint,
    grab_offset: [f32; 2],
) -> Option<Layer> {
    let p = &layer.placement;
    let (angle, reflected) = topology.transport_orientation(
        &p.surface_id,
        Vec2::new(p.x_mm as f32, p.y_mm as f32),
        &point.surface_id,
        point.point,
        (p.rotation_deg as f32).to_radians(),
    )?;
    let mut oriented = layer.clone();
    oriented.placement.rotation_deg = f64::from(angle.to_degrees());
    oriented.placement.flip_x ^= reflected;
    let offset = [
        if reflected {
            -grab_offset[0]
        } else {
            grab_offset[0]
        },
        grab_offset[1],
    ];
    let rotation = glam::Mat2::from_angle(angle);
    let center = topology.chart_offset(
        &point.surface_id,
        point.point,
        -(rotation * Vec2::from_array(offset)),
    )?;
    oriented.placement.surface_id = point.surface_id.clone();
    oriented.placement.x_mm = f64::from(center.x);
    oriented.placement.y_mm = f64::from(center.y);
    Some(oriented)
}
pub fn barycentric_world(p: Vec3, t: [Vec3; 3]) -> Option<Vec3> {
    let t = t.map(|p| p.as_dvec3());
    let p = p.as_dvec3();
    let a = t[1] - t[0];
    let b = t[2] - t[0];
    let q = p - t[0];
    let normal = a.cross(b);
    let det = normal.length_squared();
    if !det.is_finite() || det == 0. {
        return None;
    }
    let y = q.cross(b).dot(normal) / det;
    let z = a.cross(q).dot(normal) / det;
    Some(Vec3::new((1. - y - z) as f32, y as f32, z as f32))
}
pub const HANDLES: [[f32; 2]; 8] = [
    [-1., -1.],
    [0., -1.],
    [1., -1.],
    [1., 0.],
    [1., 1.],
    [0., 1.],
    [-1., 1.],
    [-1., 0.],
];

/// A frozen screen-space editing frame. The rectangle fits rendered coverage,
/// while its axes follow the layer's local orientation at its surface anchor.
#[derive(Clone, Copy, Debug)]
pub struct ScreenControlFrame {
    pub center: Vec2,
    pub x: Vec2,
    pub y: Vec2,
    pub size: Vec2,
}
impl ScreenControlFrame {
    pub fn fit(
        points: impl Iterator<Item = Vec2>,
        x: Vec2,
        y_hint: Vec2,
        minimum: f32,
    ) -> Option<Self> {
        let x = x.try_normalize().unwrap_or(Vec2::X);
        let mut y = Vec2::new(-x.y, x.x);
        if y.dot(y_hint) < 0. {
            y = -y;
        }
        let mut low = Vec2::splat(f32::INFINITY);
        let mut high = Vec2::splat(f32::NEG_INFINITY);
        for point in points.filter(|p| p.is_finite()) {
            let local = Vec2::new(point.dot(x), point.dot(y));
            low = low.min(local);
            high = high.max(local);
        }
        if !low.is_finite() || !high.is_finite() {
            return None;
        }
        let middle = (low + high) * 0.5;
        Some(Self {
            center: x * middle.x + y * middle.y,
            x,
            y,
            size: (high - low).max(Vec2::splat(minimum.max(1.))),
        })
    }
    pub fn points(self, rotation_distance: f32) -> Vec<Vec2> {
        let mut points = HANDLES
            .into_iter()
            .map(|h| {
                self.center + self.x * h[0] * self.size.x * 0.5 + self.y * h[1] * self.size.y * 0.5
            })
            .collect::<Vec<_>>();
        points.push(points[1] - self.y * rotation_distance);
        points
    }
}

pub fn transform_screen_control(
    layer: &Layer,
    frame: ScreenControlFrame,
    start: Vec2,
    current: Vec2,
    handle: usize,
    shift: bool,
) -> Layer {
    if handle == 8 {
        let a = start - frame.center;
        let b = current - frame.center;
        if a.length_squared() < 1. || b.length_squared() < 1. {
            return layer.clone();
        }
        let angle = a.perp_dot(b).atan2(a.dot(b)) * frame.x.perp_dot(frame.y);
        let mut result = layer.clone();
        result.placement.rotation_deg =
            (layer.placement.rotation_deg + f64::from(angle.to_degrees()) + 180.).rem_euclid(360.)
                - 180.;
        if shift {
            result.placement.rotation_deg = (result.placement.rotation_deg / 15.).round() * 15.;
        }
        return result;
    }
    let delta = current - start;
    let p = &layer.placement;
    let local = Vec2::new(
        delta.dot(frame.x) / frame.size.x * p.width_mm as f32,
        delta.dot(frame.y) / frame.size.y * p.height_mm as f32,
    );
    let chart = glam::Mat2::from_angle((p.rotation_deg as f32).to_radians()) * local;
    transform(layer, Vec2::ZERO, chart, Some(handle), false, shift)
}

/// Convert the physical centre shift of a resize back into the packed chart.
/// Width/height remain real millimetres, including on scaled or skewed charts.
pub fn transform_surface_control(
    topology: &itasha_surface_map::topology::Topology,
    layer: &Layer,
    frame: ScreenControlFrame,
    start: Vec2,
    current: Vec2,
    handle: usize,
    shift: bool,
) -> Option<Layer> {
    let mut result = transform_screen_control(layer, frame, start, current, handle, shift);
    let p = &layer.placement;
    let origin = Vec2::new(p.x_mm as f32, p.y_mm as f32);
    let delta = Vec2::new(
        (result.placement.x_mm - p.x_mm) as f32,
        (result.placement.y_mm - p.y_mm) as f32,
    );
    let center = topology.chart_offset(&p.surface_id, origin, delta)?;
    result.placement.x_mm = f64::from(center.x);
    result.placement.y_mm = f64::from(center.y);
    Some(result)
}
/// Screen-space hit areas overlap on small or foreshortened graphics.
/// Prefer the nearest control instead of the first control in drawing order.
pub fn nearest_handle(points: &[Vec2], pointer: Vec2, radius: f32) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| (index, point.distance_squared(pointer)))
        .filter(|(_, distance)| *distance < radius * radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}
pub fn local(layer: &Layer, point: Vec2) -> Vec2 {
    let p = &layer.placement;
    let delta = Vec2::new(point.x - p.x_mm as f32, point.y - p.y_mm as f32);
    glam::Mat2::from_angle(-(p.rotation_deg as f32).to_radians()) * delta
}
pub fn transform(
    layer: &Layer,
    start: Vec2,
    current: Vec2,
    handle: Option<usize>,
    rotate: bool,
    shift: bool,
) -> Layer {
    let mut result = layer.clone();
    let p = &mut result.placement;
    let dx = current.x - start.x;
    let delta = Vec2::new(dx, current.y - start.y);
    if rotate {
        let a = local(layer, start);
        let b = local(layer, current);
        let angle = p.rotation_deg + (b.y.atan2(b.x) - a.y.atan2(a.x)).to_degrees() as f64;
        p.rotation_deg = (angle + 180.).rem_euclid(360.) - 180.;
        if shift {
            p.rotation_deg = (p.rotation_deg / 15.).round() * 15.;
        }
    } else if let Some(index) = handle {
        let h = Vec2::from(HANDLES[index]);
        let d = glam::Mat2::from_angle(-(p.rotation_deg as f32).to_radians()) * delta;
        let old = Vec2::new(p.width_mm as f32, p.height_mm as f32);
        let mut size = Vec2::new(
            if h.x != 0. {
                (old.x + d.x * h.x).clamp(1., 10000.)
            } else {
                old.x
            },
            if h.y != 0. {
                (old.y + d.y * h.y).clamp(1., 10000.)
            } else {
                old.y
            },
        );
        if p.aspect_ratio_locked || (shift && h.x != 0. && h.y != 0.) {
            let sx = 1. + d.x * h.x / old.x;
            let sy = 1. + d.y * h.y / old.y;
            let factor = if h.x == 0. {
                sy
            } else if h.y == 0. || !p.aspect_ratio_locked || (sx - 1.).abs() >= (sy - 1.).abs() {
                sx
            } else {
                sy
            };
            // Clamp the common factor, not each axis independently: extreme
            // aspect ratios must remain exact at both supported size limits.
            size = old
                * factor.clamp(
                    (1. / old.x).max(1. / old.y),
                    (10000. / old.x).min(10000. / old.y),
                );
        }
        let offset =
            glam::Mat2::from_angle((p.rotation_deg as f32).to_radians()) * ((size - old) * h * 0.5);
        p.x_mm += offset.x as f64;
        p.y_mm += offset.y as f64;
        p.width_mm = size.x as f64;
        p.height_mm = size.y as f64;
    } else {
        p.x_mm += delta.x as f64;
        p.y_mm += delta.y as f64;
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use itasha_contracts::{LayerSource, Placement, Shape};

    #[test]
    fn picking_retains_positive_submicrometre_fragments() {
        let points = [
            Vec3::new(1., 1., 1.),
            Vec3::new(1.0000001, 1., 1.),
            Vec3::new(1., 1.0000001, 1.),
        ];
        for (point, expected) in points.into_iter().zip([Vec3::X, Vec3::Y, Vec3::Z]) {
            assert_eq!(barycentric_world(point, points), Some(expected));
        }
        assert!(barycentric_world(points[0], [points[0], points[1], points[1]]).is_none());
    }

    #[test]
    fn normal_rays_can_select_small_positive_faces_below_the_old_determinant_cutoff() {
        let node = Node::build(vec![Triangle {
            p: [
                Vec3::new(1., 1., 1.),
                Vec3::new(1.00004, 1., 1.),
                Vec3::new(1., 1.00008, 1.),
            ],
            surface: Some(0),
            id: test_triangle_id(0),
        }]);
        let mut best = None;
        node.hit(Vec3::new(1.00001, 1.00002, 2.), -Vec3::Z, &mut best);
        let hit = best.expect("normal ray must reach a retained microscopic face");
        assert!((hit.0 - 1.).abs() < 1e-6);
        assert_eq!(hit.1, Some(0));
        assert!(hit.4.min_element() > 0.2);
        let mut parallel = None;
        node.hit(Vec3::new(1., 1., 2.), Vec3::X, &mut parallel);
        assert!(parallel.is_none());
    }

    fn control_layer() -> Layer {
        Layer {
            id: uuid::Uuid::nil(),
            name: "controls".into(),
            visible: true,
            locked: false,
            opacity: 1.,
            placement: Placement {
                anchor: None,
                surface_id: "su7.front-left-door".into(),
                cross_surface: true,
                flip_x: false,
                aspect_ratio_locked: false,
                profile_version: 2,
                x_mm: 250.,
                y_mm: 300.,
                width_mm: 200.,
                height_mm: 100.,
                rotation_deg: 35.,
            },
            source: LayerSource::Shape {
                shape: Shape::Rectangle,
                fill: "#ffffff".into(),
            },
        }
    }
    #[test]
    fn clipped_and_curved_coverage_always_has_orthogonal_non_crossing_controls() {
        let cloud = [
            Vec2::new(-180., 24.),
            Vec2::new(-100., 150.),
            Vec2::new(40., 5.),
            Vec2::new(80., 75.),
            Vec2::new(32., -2.),
            Vec2::splat(f32::NAN),
        ];
        for handedness in [-1., 1.] {
            let frame = ScreenControlFrame::fit(
                cloud.into_iter(),
                Vec2::new(0.1, 0.9),
                Vec2::new(handedness, 0.),
                12.,
            )
            .unwrap();
            assert!(frame.x.dot(frame.y).abs() < 1e-6);
            let p = frame.points(26.);
            assert_eq!(p.len(), 9);
            assert!(p.iter().all(|v| v.is_finite()));
            assert!((p[0] + p[4] - frame.center * 2.).length() < 1e-4);
            assert!((p[2] + p[6] - frame.center * 2.).length() < 1e-4);
            let mut turn = 0.;
            for i in [0, 2, 4, 6] {
                let a = p[(i + 2) % 8] - p[i];
                let b = p[(i + 4) % 8] - p[(i + 2) % 8];
                assert!(a.dot(b).abs() < 0.01);
                let sign = a.perp_dot(b).signum();
                if turn != 0. {
                    assert_eq!(turn, sign);
                }
                turn = sign;
            }
            for point in cloud.into_iter().filter(|p| p.is_finite()) {
                let d = point - frame.center;
                assert!(d.dot(frame.x).abs() <= frame.size.x * 0.5 + 1e-4);
                assert!(d.dot(frame.y).abs() <= frame.size.y * 0.5 + 1e-4);
            }
        }
    }
    #[test]
    fn frozen_screen_controls_resize_and_rotate_with_mirrored_camera_axes() {
        let layer = control_layer();
        for sign in [-1., 1.] {
            let frame = ScreenControlFrame {
                center: Vec2::new(300., 200.),
                x: Vec2::X,
                y: Vec2::Y * sign,
                size: Vec2::new(160., 80.),
            };
            let points = frame.points(26.);
            let result = transform_screen_control(
                &layer,
                frame,
                points[3],
                points[3] + Vec2::new(32., 0.),
                3,
                false,
            );
            assert!((result.placement.width_mm - 240.).abs() < 1e-4);
            let fixed = |l: &Layer| {
                Vec2::new(l.placement.x_mm as f32, l.placement.y_mm as f32)
                    + glam::Mat2::from_angle((l.placement.rotation_deg as f32).to_radians())
                        * Vec2::new(-l.placement.width_mm as f32 * 0.5, 0.)
            };
            assert!(fixed(&result).distance(fixed(&layer)) < 1e-4);
            let start = points[8];
            let current = frame.center
                + glam::Mat2::from_angle(std::f32::consts::FRAC_PI_2) * (start - frame.center);
            let rotated = transform_screen_control(&layer, frame, start, current, 8, false);
            assert!((rotated.placement.rotation_deg - (35. + 90. * f64::from(sign))).abs() < 1e-4);
            assert_eq!(
                transform_screen_control(&layer, frame, start, start, 8, false),
                layer
            );
        }
    }
    #[test]
    fn ratio_lock_preserves_opposite_anchors_for_all_handles_and_size_limits() {
        let mut layer = control_layer();
        layer.placement.aspect_ratio_locked = true;
        let r = glam::Mat2::from_angle(35_f32.to_radians());
        for (i, h) in HANDLES.into_iter().enumerate() {
            let h = Vec2::from_array(h);
            for amount in [30., -500., 50000.] {
                let result = transform(&layer, Vec2::ZERO, r * (h * amount), Some(i), false, false);
                assert!((result.placement.width_mm / result.placement.height_mm - 2.).abs() < 1e-5);
                assert!((1. ..=10000.).contains(&result.placement.width_mm));
                assert!((1. ..=10000.).contains(&result.placement.height_mm));
                let fixed = |l: &Layer| {
                    Vec2::new(l.placement.x_mm as f32, l.placement.y_mm as f32)
                        - r * (h
                            * Vec2::new(l.placement.width_mm as f32, l.placement.height_mm as f32)
                            * 0.5)
                };
                assert!(fixed(&result).distance(fixed(&layer)) < 0.001);
            }
        }
    }
    #[test]
    fn overlapping_control_targets_select_nearest_handle() {
        let points = [
            Vec2::new(10., 10.),
            Vec2::new(12., 11.),
            Vec2::new(14., 12.),
        ];
        assert_eq!(nearest_handle(&points, points[2], 12.), Some(2));
        assert_eq!(nearest_handle(&points, Vec2::new(11.8, 11.), 12.), Some(1));
        assert_eq!(nearest_handle(&points, Vec2::new(40., 40.), 12.), None);
    }
    #[test]
    fn nearest_unpaintable_geometry_occludes_paint() {
        let root = Node::build(vec![
            Triangle {
                p: [
                    Vec3::new(-1., -1., 0.),
                    Vec3::new(1., -1., 0.),
                    Vec3::new(0., 1., 0.),
                ],
                surface: Some(0),
                id: test_triangle_id(0),
            },
            Triangle {
                p: [
                    Vec3::new(-1., -1., 1.),
                    Vec3::new(1., -1., 1.),
                    Vec3::new(0., 1., 1.),
                ],
                surface: None,
                id: test_triangle_id(1),
            },
        ]);
        let mut best = None;
        root.hit(Vec3::new(0., 0., 3.), -Vec3::Z, &mut best);
        assert_eq!(best.unwrap().1, None);
    }
    fn test_triangle_id(index: u32) -> itasha_surface_map::TriangleId {
        itasha_surface_map::TriangleId {
            source_mesh: "test".into(),
            primitive: 0,
            source_triangle: index,
            fragment: 0,
        }
    }

    #[test]
    fn moving_anchor_to_another_paint_chart_preserves_grab_offset_and_layer_size() {
        let layer = Layer {
            id: uuid::Uuid::nil(),
            name: "图层1".into(),
            visible: true,
            locked: false,
            opacity: 1.,
            placement: Placement {
                anchor: None,
                surface_id: "su7.front-right-door".into(),
                cross_surface: false,
                flip_x: false,
                aspect_ratio_locked: false,
                profile_version: 2,
                x_mm: 100.,
                y_mm: 200.,
                width_mm: 320.,
                height_mm: 180.,
                rotation_deg: 90.,
            },
            source: LayerSource::Shape {
                shape: Shape::Rectangle,
                fill: "#ffffff".into(),
            },
        };
        let point = SurfaceEditPoint {
            surface_id: "su7.front-right-fender".into(),
            point: Vec2::new(500., 700.),
        };
        let moved = move_anchor(&layer, &point, [30., -20.]);
        assert_eq!(moved.placement.surface_id, point.surface_id);
        assert!((moved.placement.x_mm - 480.).abs() < 1e-5);
        assert!((moved.placement.y_mm - 670.).abs() < 1e-5);
        assert_eq!(moved.placement.width_mm, layer.placement.width_mm);
        assert_eq!(moved.placement.height_mm, layer.placement.height_mm);
        assert_eq!(moved.source, layer.source);
        // A propagated fragment can have unrelated paint-chart coordinates.
        // Its source-image UV, not its paint point, determines the grabbed spot.
        let uv = Vec2::new(0.25, 0.8);
        let offset = image_grab_offset(&layer, uv);
        assert!(offset.distance(Vec2::new(-80., 54.)) < 1e-5);
        let mut flipped = layer.clone();
        flipped.placement.flip_x = true;
        assert_eq!(image_grab_offset(&flipped, uv), offset);
        let moved = move_anchor(&flipped, &point, offset.to_array());
        let angle = (moved.placement.rotation_deg as f32).to_radians();
        let recovered = Vec2::new(moved.placement.x_mm as f32, moved.placement.y_mm as f32)
            + glam::Mat2::from_angle(angle) * offset;
        assert!(recovered.distance(point.point) < 1e-4);
        assert!(moved.placement.flip_x);
    }

    #[test]
    fn physical_grab_and_resize_do_not_inherit_chart_stretch_or_shear() {
        use itasha_surface_map::{PaintTriangle, topology::Topology};
        let chart = |p: Vec2| Vec2::new(3. * p.x + 0.6 * p.y, -0.2 * p.x + 0.45 * p.y);
        let corners = [Vec2::ZERO, Vec2::new(1000., 0.), Vec2::new(0., 1000.)];
        let topology = Topology::new(
            vec![PaintTriangle {
                id: test_triangle_id(0),
                part_id: "su7.front-left-door".into(),
                region_id: "exterior".into(),
                chart_id: "skewed".into(),
                world: corners.map(|p| [p.x * 0.001, p.y * 0.001, 0.]),
                paint_uv: corners.map(|p| chart(p).to_array()),
                source_barycentric: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            }],
            &[],
        );
        let mut layer = control_layer();
        layer.placement.surface_id = "su7.front-left-door".into();
        let anchor = chart(Vec2::new(200., 200.));
        layer.placement.x_mm = f64::from(anchor.x);
        layer.placement.y_mm = f64::from(anchor.y);
        let point = SurfaceEditPoint {
            surface_id: layer.placement.surface_id.clone(),
            point: chart(Vec2::new(400., 300.)),
        };
        let offset = Vec2::new(30., -20.);
        let moved = transport_anchor(&topology, &layer, &point, offset.to_array()).unwrap();
        let moved_center = Vec2::new(moved.placement.x_mm as f32, moved.placement.y_mm as f32);
        let (world, u, v) = topology
            .tangent_frame(&point.surface_id, moved_center)
            .unwrap();
        let angle = (moved.placement.rotation_deg as f32).to_radians();
        let local_offset = glam::Mat2::from_angle(angle) * offset;
        let grabbed = world + u * local_offset.x + v * local_offset.y;
        assert!(grabbed.distance(Vec3::new(0.4, 0.3, 0.)) < 1e-6);
        assert_eq!(moved.placement.width_mm, layer.placement.width_mm);
        assert_eq!(moved.placement.height_mm, layer.placement.height_mm);

        let frame = ScreenControlFrame {
            center: Vec2::new(300., 200.),
            x: Vec2::X,
            y: Vec2::Y,
            size: Vec2::new(160., 80.),
        };
        let start = frame.points(26.)[3];
        let resized = transform_surface_control(
            &topology,
            &layer,
            frame,
            start,
            start + Vec2::new(32., 0.),
            3,
            false,
        )
        .unwrap();
        let fixed = |l: &Layer| {
            let center = Vec2::new(l.placement.x_mm as f32, l.placement.y_mm as f32);
            let (origin, u, v) = topology
                .tangent_frame(&l.placement.surface_id, center)
                .unwrap();
            let (sin, cos) = (l.placement.rotation_deg as f32).to_radians().sin_cos();
            origin - (u * cos + v * sin) * l.placement.width_mm as f32 * 0.5
        };
        assert!(
            fixed(&resized).distance(fixed(&layer)) < 1e-6,
            "resizing must keep the opposite physical edge fixed on a skewed chart"
        );
    }
}
