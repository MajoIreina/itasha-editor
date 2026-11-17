//! Stable, renderer-independent identities and reversible coordinates for paint surfaces.
pub mod gesture;
pub mod topology;
pub mod transition;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub const MAP_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TriangleId {
    pub source_mesh: String,
    pub primitive: u32,
    pub source_triangle: u32,
    pub fragment: u64,
}

impl TriangleId {
    pub fn stable_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.source_mesh, self.primitive, self.source_triangle, self.fragment
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaintTriangle {
    pub id: TriangleId,
    pub part_id: String,
    pub region_id: String,
    pub chart_id: String,
    pub world: [[f32; 3]; 3],
    pub source_barycentric: [[f32; 3]; 3],
    /// Per-chart coordinates used by the 2D editor.
    pub paint_uv: [[f32; 2]; 3],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverageRect {
    pub center: Vec2,
    pub size: Vec2,
    pub rotation_rad: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageFragment {
    #[serde(skip_serializing)]
    pub triangle: TriangleId,
    pub part_id: String,
    pub region_id: String,
    #[serde(skip_serializing)]
    pub world: Vec<[f32; 3]>,
    pub image_uv: Vec<[f32; 2]>,
    pub paint_uv: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeRef {
    pub triangle: TriangleId,
    pub edge: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceHinge {
    pub from_source_edge: u8,
    pub to_source_edge: u8,
    /// Original GLB shared edge, ordered like from_source_edge. Retained masks
    /// need not touch it; this relation never adds triangles to those masks.
    pub world: [[f32; 3]; 2],
    /// Reviewed common near-edge support, parameterized on the original edge.
    /// Crossing is allowed only where the image intersects one of these ranges.
    pub support_intervals: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeConnection {
    pub id: String,
    pub from: EdgeRef,
    pub to: EdgeRef,
    /// Pair endpoints in the same order, or reverse them when true.
    pub reversed: bool,
    /// Real seam gap in millimetres. The UI layout gap is never used here.
    pub physical_gap_mm: f32,
    pub propagation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hinge: Option<SourceHinge>,
}

/// Recover the original triangle from a stable retained fragment. This is an
/// affine inverse in normalized source barycentrics, not a spatial neighbour fit.
pub(crate) fn original_face_world(t: &PaintTriangle) -> Option<[glam::DVec3; 3]> {
    use glam::{DMat2, DVec2, DVec3};
    let s = t.source_barycentric.map(|b| {
        let sum = b.into_iter().map(f64::from).sum::<f64>();
        DVec2::new(f64::from(b[1]) / sum, f64::from(b[2]) / sum)
    });
    let matrix = DMat2::from_cols(s[1] - s[0], s[2] - s[0]);
    let w = t.world.map(|p| DVec3::from_array(p.map(f64::from)));
    let height = (w[1] - w[0]).cross(w[2] - w[0]).length()
        / (w[1] - w[0])
            .length()
            .max((w[2] - w[0]).length())
            .max((w[2] - w[1]).length());
    if matrix.determinant().abs() <= 1e-12 || !height.is_finite() || height <= 2e-6 {
        return None;
    }
    let inverse = matrix.inverse();
    let result = [DVec2::ZERO, DVec2::X, DVec2::Y].map(|p| {
        let q = inverse * (p - s[0]);
        w[0] + (w[1] - w[0]) * q.x + (w[2] - w[0]) * q.y
    });
    result.iter().all(|p| p.is_finite()).then_some(result)
}

pub(crate) fn source_hinge_geometry(
    edge: &EdgeConnection,
    from: &PaintTriangle,
    to: &PaintTriangle,
) -> Option<([glam::DVec3; 3], [glam::DVec3; 3])> {
    let h = edge.source_hinge.as_ref()?;
    if h.from_source_edge > 2
        || h.to_source_edge > 2
        || from.part_id != to.part_id
        || from.id.source_mesh != to.id.source_mesh
        || from.id.primitive != to.id.primitive
        || from.id.source_triangle == to.id.source_triangle
        || edge.physical_gap_mm != 0.
        || !edge.propagation
        || h.support_intervals.is_empty()
        || h.support_intervals
            .iter()
            .any(|[a, b]| !a.is_finite() || !b.is_finite() || *a < 0. || *b > 1. || a >= b)
    {
        return None;
    }
    let a = original_face_world(from)?;
    let b = original_face_world(to)?;
    let hinge = h.world.map(|p| glam::DVec3::from_array(p.map(f64::from)));
    let direction = (hinge[1] - hinge[0]).try_normalize()?;
    if !hinge.iter().all(|p| p.is_finite()) {
        return None;
    }
    for (i, original, reversed) in [
        (h.from_source_edge, a, false),
        (h.to_source_edge, b, edge.reversed),
    ] {
        let mut endpoints = [original[i as usize], original[(i as usize + 1) % 3]];
        if reversed {
            endpoints.reverse();
        }
        if endpoints
            .into_iter()
            .zip(hinge)
            .any(|(p, q)| p.distance(q) > 5e-6)
        {
            return None;
        }
    }
    let inward = |p: glam::DVec3| {
        let q = p - hinge[0];
        (q - direction * q.dot(direction)).try_normalize()
    };
    // These authored relations are ordinary exterior bends, not coincident
    // sheets or a folded-back return. The generator separately proves GLB adjacency.
    if inward(a[(h.from_source_edge as usize + 2) % 3])?
        .dot(inward(b[(h.to_source_edge as usize + 2) % 3])?)
        >= 0.
    {
        return None;
    }
    Some((a, b))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaintSurfaceMap {
    pub mapping_version: u32,
    pub model_fingerprint: String,
    pub paint_fingerprint: String,
    pub triangles: Vec<PaintTriangle>,
    pub connections: Vec<EdgeConnection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfacePoint {
    pub triangle: TriangleId,
    pub barycentric: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MappedPoint {
    pub world: Vec3,
    pub paint_uv: Vec2,
    pub part_id: String,
}

impl PaintSurfaceMap {
    pub fn validate(&self) -> Result<(), String> {
        if self.mapping_version != MAP_VERSION
            || self.model_fingerprint.is_empty()
            || self.paint_fingerprint.is_empty()
            || self.paint_fingerprint != self.content_fingerprint()
        {
            return Err("surface map version or fingerprint is missing/unsupported".into());
        }
        let mut ids = HashSet::with_capacity(self.triangles.len());
        for triangle in &self.triangles {
            if triangle.part_id.is_empty()
                || triangle.region_id.is_empty()
                || triangle.chart_id.is_empty()
                || !ids.insert(triangle.id.clone())
            {
                return Err(format!(
                    "invalid or duplicate paint triangle {}",
                    triangle.id.stable_key()
                ));
            }
            let w = triangle.world.map(|p| Vec3::from_array(p).as_dvec3());
            let uv = triangle.paint_uv.map(Vec2::from_array);
            if triangle.source_barycentric.iter().any(|b| {
                b.iter()
                    .any(|v| !v.is_finite() || *v < -0.002 || *v > 1.002)
                    || (b.iter().sum::<f32>() - 1.).abs() > 0.002
            }) {
                return Err(format!(
                    "invalid source barycentric: {}",
                    triangle.id.stable_key()
                ));
            }
            if w.iter().any(|v| !v.is_finite())
                || uv.iter().any(|v| !v.is_finite())
                || (w[1] - w[0]).cross(w[2] - w[0]).length_squared() == 0.
                || (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs() < 1e-12
            {
                return Err(format!(
                    "degenerate or non-finite paint triangle {}",
                    triangle.id.stable_key()
                ));
            }
        }
        let mut edge_ids = HashSet::new();
        for edge in &self.connections {
            if edge.id.is_empty()
                || !edge.physical_gap_mm.is_finite()
                || edge.physical_gap_mm < 0.
                || edge.from.edge > 2
                || edge.to.edge > 2
                || !ids.contains(&edge.from.triangle)
                || !ids.contains(&edge.to.triangle)
                || edge.from == edge.to
                || !edge_ids.insert(edge.id.as_str())
            {
                return Err(format!("invalid paint edge connection {}", edge.id));
            }
            let from = self
                .triangles
                .iter()
                .find(|t| t.id == edge.from.triangle)
                .unwrap();
            let to = self
                .triangles
                .iter()
                .find(|t| t.id == edge.to.triangle)
                .unwrap();
            if edge.source_hinge.is_some() {
                if source_hinge_geometry(edge, from, to).is_none() {
                    return Err(format!("invalid original source edge hinge {}", edge.id));
                }
                continue;
            }
            let edge_points = |points: &[[f32; 3]; 3], index: u8| {
                [
                    Vec3::from_array(points[index as usize]),
                    Vec3::from_array(points[(index as usize + 1) % 3]),
                ]
            };
            let a = edge_points(&from.world, edge.from.edge);
            let mut b = edge_points(&to.world, edge.to.edge);
            if edge.reversed {
                b.reverse();
            }
            let tolerance = edge.physical_gap_mm / 1000. + 0.005;
            if (a[0] - b[0]).length() > tolerance
                || (a[1] - b[1]).length() > tolerance
                || ((a[0] - a[1]).length() - (b[0] - b[1]).length()).abs() > 0.005
            {
                return Err(format!(
                    "paint edge correspondence does not match geometry: {}",
                    edge.id
                ));
            }
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> String {
        self.content_fingerprint()
    }

    fn content_fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        let mut canonical = self.clone();
        canonical.paint_fingerprint.clear();
        hash.update(
            serde_json::to_vec(&canonical).expect("surface map serialization is infallible"),
        );
        format!("{:x}", hash.finalize())
    }

    pub fn index(&self) -> Result<SurfaceMapIndex<'_>, String> {
        self.validate()?;
        let mut triangles_by_chart: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut triangle_by_id = HashMap::with_capacity(self.triangles.len());
        for (index, triangle) in self.triangles.iter().enumerate() {
            triangles_by_chart
                .entry(&triangle.chart_id)
                .or_default()
                .push(index);
            triangle_by_id.insert(&triangle.id, index);
        }
        Ok(SurfaceMapIndex {
            map: self,
            triangles_by_chart,
            triangle_by_id,
        })
    }
}

pub struct SurfaceMapIndex<'a> {
    map: &'a PaintSurfaceMap,
    triangles_by_chart: HashMap<&'a str, Vec<usize>>,
    triangle_by_id: HashMap<&'a TriangleId, usize>,
}

impl SurfaceMapIndex<'_> {
    /// Intersect artwork with only the named chart triangles. The returned mesh is the
    /// authoritative target set for picking, preview, render, and export.
    pub fn coverage(
        &self,
        chart: &str,
        rect: CoverageRect,
        allowed_parts: Option<&HashSet<String>>,
    ) -> Vec<CoverageFragment> {
        let Some(indices) = self.triangles_by_chart.get(chart) else {
            return Vec::new();
        };
        coverage_triangles(
            indices.iter().map(|index| &self.map.triangles[*index]),
            rect,
            allowed_parts,
        )
    }

    pub fn source_point(&self, point: &SurfacePoint) -> Option<MappedPoint> {
        let triangle = self
            .map
            .triangles
            .get(*self.triangle_by_id.get(&point.triangle)?)?;
        let b = point.barycentric;
        if !b.is_finite() || (b.element_sum() - 1.).abs() > 1e-4 || b.min_element() < -1e-5 {
            return None;
        }
        let world = triangle.world.map(Vec3::from_array);
        let uv = triangle.paint_uv.map(Vec2::from_array);
        Some(MappedPoint {
            world: world[0] * b.x + world[1] * b.y + world[2] * b.z,
            paint_uv: uv[0] * b.x + uv[1] * b.y + uv[2] * b.z,
            part_id: triangle.part_id.clone(),
        })
    }

    pub fn locate_uv(&self, chart: &str, point: Vec2) -> Option<SurfacePoint> {
        if !point.is_finite() {
            return None;
        }
        self.triangles_by_chart
            .get(chart)?
            .iter()
            .find_map(|index| {
                let triangle = &self.map.triangles[*index];
                let barycentric = barycentric_2d(point, triangle.paint_uv.map(Vec2::from_array))?;
                (barycentric.min_element() >= -1e-6).then(|| SurfacePoint {
                    triangle: triangle.id.clone(),
                    barycentric,
                })
            })
    }

    pub fn world_at_uv(&self, chart: &str, point: Vec2) -> Option<MappedPoint> {
        let located = self.locate_uv(chart, point)?;
        self.source_point(&located)
    }
}

pub fn coverage_triangles<'a>(
    triangles: impl Iterator<Item = &'a PaintTriangle>,
    rect: CoverageRect,
    allowed_parts: Option<&HashSet<String>>,
) -> Vec<CoverageFragment> {
    if !rect.center.is_finite()
        || !rect.size.is_finite()
        || rect.size.min_element() <= 0.
        || !rect.rotation_rad.is_finite()
    {
        return Vec::new();
    }
    let rotation = glam::Mat2::from_angle(-rect.rotation_rad);
    let half = rect.size * 0.5;
    let mut result = Vec::new();
    for triangle in triangles {
        if allowed_parts.is_some_and(|parts| !parts.contains(&triangle.part_id)) {
            continue;
        }
        let chart_points = triangle.paint_uv.map(Vec2::from_array);
        let world_points = triangle.world.map(Vec3::from_array);
        let mut polygon: Vec<(Vec3, Vec3)> = (0..3)
            .map(|i| {
                (
                    Vec3::from_array([chart_points[i].x, chart_points[i].y, 0.]),
                    world_points[i],
                )
            })
            .collect();
        for (axis, bound, greater) in [
            (0, -half.x, true),
            (0, half.x, false),
            (1, -half.y, true),
            (1, half.y, false),
        ] {
            if polygon.is_empty() {
                break;
            }
            let inside = |p: &(Vec3, Vec3)| {
                let local = rotation.mul_vec2(p.0.truncate() - rect.center);
                if greater {
                    local[axis] >= bound
                } else {
                    local[axis] <= bound
                }
            };
            let mut output = Vec::new();
            let mut previous = *polygon.last().unwrap();
            for current in polygon.iter().copied() {
                let a = inside(&previous);
                let b = inside(&current);
                if a != b {
                    let pa = rotation.mul_vec2(previous.0.truncate() - rect.center);
                    let pb = rotation.mul_vec2(current.0.truncate() - rect.center);
                    let t = (bound - pa[axis]) / (pb[axis] - pa[axis]);
                    output.push(interpolate(previous, current, t));
                }
                if b {
                    output.push(current);
                }
                previous = current;
            }
            polygon = output;
        }
        if polygon.len() < 3 {
            continue;
        }
        for i in 1..polygon.len() - 1 {
            let tri = [polygon[0], polygon[i], polygon[i + 1]];
            result.push(CoverageFragment {
                triangle: triangle.id.clone(),
                part_id: triangle.part_id.clone(),
                region_id: triangle.region_id.clone(),
                world: tri.map(|p| p.1.to_array()).to_vec(),
                image_uv: tri
                    .map(|p| {
                        let local = rotation.mul_vec2(p.0.truncate() - rect.center);
                        ((local + half) / rect.size).to_array()
                    })
                    .to_vec(),
                paint_uv: tri.map(|p| p.0.truncate().to_array()).to_vec(),
            });
        }
    }
    result
}

fn interpolate(a: (Vec3, Vec3), b: (Vec3, Vec3), t: f32) -> (Vec3, Vec3) {
    (a.0.lerp(b.0, t), a.1.lerp(b.1, t))
}

pub fn barycentric_2d(point: Vec2, tri: [Vec2; 3]) -> Option<Vec3> {
    let tri = tri.map(|p| p.as_dvec2());
    let point = point.as_dvec2();
    let a = tri[1] - tri[0];
    let b = tri[2] - tri[0];
    let d = point - tri[0];
    let det = a.perp_dot(b);
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let y = d.perp_dot(b) / det;
    let z = a.perp_dot(d) / det;
    Some(Vec3::new((1. - y - z) as f32, y as f32, z as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PaintSurfaceMap {
        let mut map = PaintSurfaceMap {
            mapping_version: MAP_VERSION,
            model_fingerprint: "model-v1".into(),
            paint_fingerprint: String::new(),
            triangles: vec![PaintTriangle {
                id: TriangleId {
                    source_mesh: "body/door".into(),
                    primitive: 2,
                    source_triangle: 41,
                    fragment: 0,
                },
                part_id: "front-door-left".into(),
                region_id: "door-outer".into(),
                chart_id: "door-chart".into(),
                world: [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.]],
                source_barycentric: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                paint_uv: [[0., 0.], [1., 0.], [0., 1.]],
            }],
            connections: vec![],
        };
        map.paint_fingerprint = map.fingerprint();
        map
    }
    #[test]
    fn chart_lookup_round_trips_to_world_and_source_identity() {
        let map = fixture();
        let index = map.index().unwrap();
        let uv = Vec2::new(0.25, 0.5);
        let location = index.locate_uv("door-chart", uv).unwrap();
        assert_eq!(location.triangle, map.triangles[0].id);
        let mapped = index.source_point(&location).unwrap();
        assert!((mapped.paint_uv - uv).length() < 1e-6);
        assert!((mapped.world - Vec3::new(0.5, 0.5, 0.)).length() < 1e-6);
        assert_eq!(location.triangle.source_triangle, 41);
        assert_eq!(mapped.part_id, "front-door-left");
    }
    #[test]
    fn positive_f32_world_fragments_are_retained_below_the_old_area_cutoff() {
        let mut map = fixture();
        map.triangles[0].world = [[1., 1., 1.], [1.0000001, 1., 1.], [1., 1.0000001, 1.]];
        let w = map.triangles[0]
            .world
            .map(|p| Vec3::from_array(p).as_dvec3());
        let cross_squared = (w[1] - w[0]).cross(w[2] - w[0]).length_squared();
        assert!(cross_squared > 0. && cross_squared < 1e-24);
        map.paint_fingerprint = map.fingerprint();
        map.validate().unwrap();
        let topology = topology::Topology::new(map.triangles.clone(), &[]);
        assert!(
            !topology
                .coverage_at("front-door-left", [0.25, 0.25], [1., 1.], 0., false)
                .is_empty()
        );
        // Actual degeneracy remains invalid, independently of area scale.
        map.triangles[0].world[2] = map.triangles[0].world[1];
        map.paint_fingerprint = map.fingerprint();
        assert!(map.validate().unwrap_err().contains("degenerate"));
    }

    #[test]
    fn invalid_uv_degeneracy_duplicate_identity_and_missing_seam_target_fail() {
        let mut map = fixture();
        map.validate().unwrap();
        map.triangles[0].paint_uv[2] = [2., 0.];
        assert!(map.validate().is_err());
        let mut map = fixture();
        map.triangles.push(map.triangles[0].clone());
        assert!(map.validate().is_err());
        let mut map = fixture();
        map.connections.push(EdgeConnection {
            id: "door-seam".into(),
            from: EdgeRef {
                triangle: map.triangles[0].id.clone(),
                edge: 0,
            },
            to: EdgeRef {
                triangle: TriangleId {
                    source_mesh: "missing".into(),
                    primitive: 0,
                    source_triangle: 1,
                    fragment: 0,
                },
                edge: 0,
            },
            reversed: false,
            physical_gap_mm: 3.,
            propagation: true,
            source_hinge: None,
        });
        assert!(map.validate().is_err());
    }
    #[test]
    fn map_fingerprint_is_repeatable_and_changes_with_seam_semantics() {
        let mut map = fixture();
        let a = map.fingerprint();
        assert_eq!(a, map.fingerprint());
        map.connections.push(EdgeConnection {
            id: "join".into(),
            from: EdgeRef {
                triangle: map.triangles[0].id.clone(),
                edge: 0,
            },
            to: EdgeRef {
                triangle: map.triangles[0].id.clone(),
                edge: 1,
            },
            reversed: false,
            physical_gap_mm: 0.,
            propagation: true,
            source_hinge: None,
        });
        assert_ne!(a, map.fingerprint());
    }

    #[test]
    fn coverage_clips_to_explicit_chart_triangles_and_emits_interpolated_artwork_uv() {
        let mut map = fixture();
        let second = PaintTriangle {
            id: TriangleId {
                source_mesh: "body/rear-door".into(),
                primitive: 0,
                source_triangle: 8,
                fragment: 0,
            },
            part_id: "rear-door-left".into(),
            region_id: "outer".into(),
            chart_id: "door-chart".into(),
            world: [[2., 0., 0.], [3., 0., 0.], [2., 1., 0.]],
            source_barycentric: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            paint_uv: [[2., 0.], [3., 0.], [2., 1.]],
        };
        map.triangles.push(second);
        map.paint_fingerprint = map.fingerprint();
        let index = map.index().unwrap();
        let result = index.coverage(
            "door-chart",
            CoverageRect {
                center: Vec2::new(0.5, 0.25),
                size: Vec2::splat(0.5),
                rotation_rad: 0.,
            },
            None,
        );
        assert!(!result.is_empty());
        assert!(result.iter().all(|f| f.part_id == "front-door-left"));
        assert!(
            result
                .iter()
                .flat_map(|f| &f.image_uv)
                .all(|uv| uv.iter().all(|v| (-1e-6..=1.000001).contains(v)))
        );
        let allowed = HashSet::from(["rear-door-left".to_string()]);
        assert!(
            index
                .coverage(
                    "door-chart",
                    CoverageRect {
                        center: Vec2::new(0.5, 0.25),
                        size: Vec2::splat(0.5),
                        rotation_rad: 0.
                    },
                    Some(&allowed)
                )
                .is_empty()
        );
    }
}
