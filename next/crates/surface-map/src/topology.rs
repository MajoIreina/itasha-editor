//! Local metric unfolding over surface adjacency. Screen layout is never input.
use crate::{CoverageFragment, CoverageRect, EdgeConnection, PaintTriangle, barycentric_2d};
use glam::{DMat2, DVec2, DVec3, Vec2, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy)]
struct Link {
    target: usize,
    edge: usize,
    target_edge: usize,
    reversed: bool,
    explicit: bool,
}

pub struct Topology {
    pub triangles: Vec<PaintTriangle>,
    links: Vec<Vec<Link>>,
    components: Vec<usize>,
    // Multiplier relating each triangle's vertex winding to its component's
    // consistently oriented exterior sheet. Endpoint order is not sheet order.
    exterior_winding: Vec<f64>,
}
type Position = [i64; 3];
type EdgeKey = (String, Position, Position);
type EdgeEntry = (usize, usize, bool);

// A single conversion relates physical millimetres to the anchor's existing
// chart. All surface transport stays in the physical domain; chart distortion
// must never be reintroduced at every triangle edge.
#[derive(Clone, Copy)]
struct ChartFrame {
    basis: DMat2,
    physical_origin: DVec2,
    chart_origin: DVec2,
}

impl ChartFrame {
    fn new(physical: [DVec2; 3], chart: [DVec2; 3]) -> Self {
        Self {
            basis: DMat2::from_cols(chart[1] - chart[0], chart[2] - chart[0])
                * DMat2::from_cols(physical[1] - physical[0], physical[2] - physical[0]).inverse(),
            physical_origin: physical[0],
            chart_origin: chart[0],
        }
    }

    fn map(&self, points: [DVec2; 3]) -> [DVec2; 3] {
        points.map(|p| self.chart_origin + self.basis * (p - self.physical_origin))
    }

    fn physical_point(&self, point: Vec2) -> DVec2 {
        self.physical_origin + self.basis.inverse() * (point.as_dvec2() - self.chart_origin)
    }
}

fn world_points(triangle: &PaintTriangle) -> [DVec3; 3] {
    triangle.world.map(|p| DVec3::from_array(p.map(f64::from)))
}

fn physical_triangle(triangle: &PaintTriangle) -> [DVec2; 3] {
    let [a, b, c] = world_points(triangle);
    let edge = b - a;
    let length = edge.length();
    [
        DVec2::ZERO,
        DVec2::new(length * 1000., 0.),
        DVec2::new(
            (c - a).dot(edge / length) * 1000.,
            edge.cross(c - a).length() / length * 1000.,
        ),
    ]
}

/// Clip in double precision while retaining source barycentrics. A narrow triangle
/// can collapse after converting unfolded coordinates to f32, so recovering its
/// barycentrics by inverting that rounded triangle is both lossy and unsafe.
fn coverage_in_domain(
    triangle: &PaintTriangle,
    domain: [DVec2; 3],
    rect: CoverageRect,
) -> Vec<CoverageFragment> {
    let center = rect.center.as_dvec2();
    let size = rect.size.as_dvec2();
    let half = size * 0.5;
    let rotation = DMat2::from_angle(-(rect.rotation_rad as f64));
    let mut polygon: Vec<_> = domain
        .into_iter()
        .zip([DVec3::X, DVec3::Y, DVec3::Z])
        .map(|(point, barycentric)| (rotation * (point - center), barycentric))
        .collect();
    for (axis, bound, greater) in [
        (0, -half.x, true),
        (0, half.x, false),
        (1, -half.y, true),
        (1, half.y, false),
    ] {
        let Some(&last) = polygon.last() else {
            break;
        };
        let inside = |point: DVec2| {
            if greater {
                point[axis] >= bound
            } else {
                point[axis] <= bound
            }
        };
        let mut previous = last;
        let mut output = Vec::with_capacity(polygon.len() + 1);
        for current in polygon.iter().copied() {
            let a = inside(previous.0);
            let b = inside(current.0);
            if a != b {
                let t = (bound - previous.0[axis]) / (current.0[axis] - previous.0[axis]);
                output.push((previous.0.lerp(current.0, t), previous.1.lerp(current.1, t)));
            }
            if b {
                output.push(current);
            }
            previous = current;
        }
        polygon = output;
    }
    let world = world_points(triangle);
    let paint = triangle.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
    let mut result = Vec::with_capacity(polygon.len().saturating_sub(2));
    for i in 1..polygon.len().saturating_sub(1) {
        let vertices = [polygon[0], polygon[i], polygon[i + 1]];
        result.push(CoverageFragment {
            triangle: triangle.id.clone(),
            part_id: triangle.part_id.clone(),
            region_id: triangle.region_id.clone(),
            world: vertices
                .map(|(_, b)| {
                    (world[0] * b.x + world[1] * b.y + world[2] * b.z)
                        .as_vec3()
                        .to_array()
                })
                .to_vec(),
            image_uv: vertices
                .map(|(point, _)| ((point + half) / size).as_vec2().to_array())
                .to_vec(),
            paint_uv: vertices
                .map(|(_, b)| {
                    (paint[0] * b.x + paint[1] * b.y + paint[2] * b.z)
                        .as_vec2()
                        .to_array()
                })
                .to_vec(),
        });
    }
    result
}
fn position(p: [f32; 3]) -> Position {
    p.map(|v| (v as f64 * 1_000_000.).round() as i64)
}

impl Topology {
    pub fn new(triangles: Vec<PaintTriangle>, connections: &[EdgeConnection]) -> Self {
        let mut links = vec![vec![]; triangles.len()];
        let mut edges: HashMap<EdgeKey, Vec<EdgeEntry>> = HashMap::new();
        for (i, t) in triangles.iter().enumerate() {
            for edge in 0..3 {
                let a = position(t.world[edge]);
                let b = position(t.world[(edge + 1) % 3]);
                let forward = a <= b;
                let key = (t.part_id.clone(), a.min(b), a.max(b));
                edges.entry(key).or_default().push((i, edge, forward));
            }
        }
        for entries in edges.values() {
            // Non-manifold edges require explicit authoring; never choose an arbitrary sheet.
            if let [(a, ae, af), (b, be, bf)] = entries.as_slice() {
                links[*a].push(Link {
                    target: *b,
                    edge: *ae,
                    target_edge: *be,
                    reversed: af != bf,
                    explicit: false,
                });
                links[*b].push(Link {
                    target: *a,
                    edge: *be,
                    target_edge: *ae,
                    reversed: af != bf,
                    explicit: false,
                });
            }
        }
        let ids: HashMap<_, _> = triangles
            .iter()
            .enumerate()
            .map(|(i, t)| (&t.id, i))
            .collect();
        for c in connections.iter().filter(|c| c.propagation) {
            if c.from.edge > 2 || c.to.edge > 2 {
                continue;
            }
            let (Some(&a), Some(&b)) = (ids.get(&c.from.triangle), ids.get(&c.to.triangle)) else {
                continue;
            };
            links[a].push(Link {
                target: b,
                edge: c.from.edge as usize,
                target_edge: c.to.edge as usize,
                reversed: c.reversed,
                explicit: true,
            });
            links[b].push(Link {
                target: a,
                edge: c.to.edge as usize,
                target_edge: c.from.edge as usize,
                reversed: c.reversed,
                explicit: true,
            });
        }
        for neighbors in &mut links {
            neighbors.sort_by_key(|l| (l.edge, l.target, l.target_edge));
        }
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for p in triangles.iter().flat_map(world_points) {
            min = min.min(p);
            max = max.max(p);
        }
        let reference = (min + max) * 0.5;
        let reference_scale = (max - min).length().max(1.);
        let mut components = vec![usize::MAX; triangles.len()];
        let mut exterior_winding = vec![0.; triangles.len()];
        for seed in 0..triangles.len() {
            if components[seed] != usize::MAX {
                continue;
            }
            components[seed] = seed;
            exterior_winding[seed] = 1.;
            let mut queue = VecDeque::from([seed]);
            let mut members = Vec::new();
            let mut normal_sum = DVec3::ZERO;
            let mut radial_score = 0.;
            let mut area = 0.;
            while let Some(index) = queue.pop_front() {
                members.push(index);
                let [a, b, c] = world_points(&triangles[index]);
                let normal = (b - a).cross(c - a) * exterior_winding[index];
                normal_sum += normal;
                radial_score += normal.dot((a + b + c) / 3. - reference);
                area += normal.length();
                for link in &links[index] {
                    if components[link.target] == usize::MAX
                        && triangles[link.target].part_id == triangles[seed].part_id
                    {
                        components[link.target] = seed;
                        exterior_winding[link.target] =
                            exterior_winding[index] * if link.reversed { 1. } else { -1. };
                        queue.push_back(link.target);
                    }
                }
            }
            // A vehicle paint component is an exterior sheet. Orient the whole
            // connected component using its area-weighted outward direction,
            // rather than the normal of a tiny rolled/clipped seam triangle.
            // Planar fixtures with their reference point on the sheet use a
            // deterministic dominant-axis convention instead.
            let orientation = if radial_score.abs() > area * reference_scale * 1e-6 {
                radial_score.signum()
            } else {
                let axis = (0..3)
                    .max_by(|&a, &b| normal_sum[a].abs().total_cmp(&normal_sum[b].abs()))
                    .unwrap();
                if normal_sum[axis] < 0. { -1. } else { 1. }
            };
            for index in members {
                exterior_winding[index] *= orientation;
            }
        }
        Self {
            triangles,
            links,
            components,
            exterior_winding,
        }
    }

    /// Carry image orientation through the shortest rotation between exterior
    /// tangent planes. Chart packing rotation/reflection must not rotate the image.
    pub fn transport_orientation(
        &self,
        from: &str,
        at: glam::Vec2,
        to: &str,
        point: glam::Vec2,
        angle: f32,
    ) -> Option<(f32, bool)> {
        let (a, _) = self.locate(from, at)?;
        let (b, _) = self.locate(to, point)?;
        if self.triangles[a].chart_id == self.triangles[b].chart_id {
            return Some((angle, false));
        }
        let frame = |i: usize| {
            let t = &self.triangles[i];
            let w = t.world.map(glam::Vec3::from_array);
            let uv = t.paint_uv.map(glam::Vec2::from_array);
            let e = uv[1] - uv[0];
            let f = uv[2] - uv[0];
            let det = e.perp_dot(f);
            let u = ((w[1] - w[0]) * f.y - (w[2] - w[0]) * e.y) / det;
            let v = ((w[2] - w[0]) * e.x - (w[1] - w[0]) * f.x) / det;
            let n =
                ((w[1] - w[0]).cross(w[2] - w[0]) * self.exterior_winding[i] as f32).normalize();
            (u, v, n)
        };
        let (u, v, n) = frame(a);
        let (tu, tv, tn) = frame(b);
        // At an antipodal normal the shortest rotation has no unique axis.
        // Use the source image vertical axis as a deterministic half-turn axis.
        let vertical = (v * angle.cos() - u * angle.sin()).normalize();
        let turn = if n.dot(tn) < -0.99999 {
            glam::Quat::from_axis_angle(vertical, std::f32::consts::PI)
        } else {
            glam::Quat::from_rotation_arc(n, tn)
        };
        let y = turn * vertical;
        let aa = tu.dot(tu);
        let ab = tu.dot(tv);
        let bb = tv.dot(tv);
        let det = aa * bb - ab * ab;
        if !det.is_finite() || det <= 1e-24 {
            return None;
        }
        let q = glam::Vec2::new(
            (bb * tu.dot(y) - ab * tv.dot(y)) / det,
            (aa * tv.dot(y) - ab * tu.dot(y)) / det,
        );
        let rotation = (-q.x).atan2(q.y);
        let reflected = u.cross(v).dot(n).signum() != tu.cross(tv).dot(tn).signum();
        rotation.is_finite().then_some((rotation, reflected))
    }

    pub fn coverage_at(
        &self,
        part: &str,
        center: [f32; 2],
        size: [f32; 2],
        rotation_rad: f32,
        adjacent: bool,
    ) -> Vec<CoverageFragment> {
        self.coverage(
            part,
            CoverageRect {
                center: Vec2::from_array(center),
                size: Vec2::from_array(size),
                rotation_rad,
            },
            adjacent,
        )
    }
    pub fn locate(&self, part: &str, point: Vec2) -> Option<(usize, Vec3)> {
        self.triangles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.part_id == part)
            .find_map(|(i, t)| {
                let b = barycentric_2d(point, t.paint_uv.map(Vec2::from_array))?;
                (b.min_element() >= -1e-5).then_some((i, b))
            })
    }

    /// Transport through actual triangle edges and authored seam links only.
    /// Deterministic breadth-first paths also keep computational UV cuts connected.
    pub fn coverage(
        &self,
        part: &str,
        rect: CoverageRect,
        adjacent: bool,
    ) -> Vec<CoverageFragment> {
        if !rect.center.is_finite()
            || !rect.size.is_finite()
            || rect.size.min_element() <= 0.
            || !rect.rotation_rad.is_finite()
        {
            return vec![];
        }
        let Some((seed, _)) = self.locate(part, rect.center) else {
            return vec![];
        };
        let seed_domain = physical_triangle(&self.triangles[seed]);
        let frame = ChartFrame::new(
            seed_domain,
            self.triangles[seed]
                .paint_uv
                .map(|p| Vec2::from_array(p).as_dvec2()),
        );
        let physical_center = frame.physical_point(rect.center);
        let inverse = frame.basis.inverse();
        let rotation = DMat2::from_angle(rect.rotation_rad as f64);
        let radius = [
            DVec2::new(-1., -1.),
            DVec2::new(-1., 1.),
            DVec2::new(1., -1.),
            DVec2::new(1., 1.),
        ]
        .into_iter()
        .map(|corner| (inverse * rotation * (corner * rect.size.as_dvec2() * 0.5)).length())
        .fold(0_f64, f64::max);
        if !physical_center.is_finite() || !radius.is_finite() || radius <= 0. {
            return vec![];
        }
        if std::env::var_os("ITASHA_DEBUG_FRAME").is_some() {
            let t = &self.triangles[seed];
            let area = |p: [[f32; 2]; 3]| {
                let a = glam::Vec2::from_array(p[1]) - glam::Vec2::from_array(p[0]);
                let b = glam::Vec2::from_array(p[2]) - glam::Vec2::from_array(p[0]);
                a.perp_dot(b).abs() * 0.5
            };
            eprintln!(
                "coverage part={} seed={} chart={} tri={} world_edges={:?} uv_edges={:?} uv_area={} radius={}",
                part,
                seed,
                t.chart_id,
                t.id.stable_key(),
                [0, 1, 2].map(|k| (glam::Vec3::from_array(t.world[(k + 1) % 3])
                    - glam::Vec3::from_array(t.world[k]))
                .length()),
                [0, 1, 2].map(|k| (glam::Vec2::from_array(t.paint_uv[(k + 1) % 3])
                    - glam::Vec2::from_array(t.paint_uv[k]))
                .length()),
                area(t.paint_uv),
                radius
            );
        }
        let mut visited = vec![false; self.triangles.len()];
        let mut expanded_components = HashSet::new();
        let mut pending = vec![(seed, seed_domain, 0_f64)];
        let mut result = Vec::new();
        // Finish each connected surface before crossing a physical seam. Multiple seam
        // entries must not establish competing unfolding frames inside one surface,
        // and a path through another part must never change the anchor surface.
        while !pending.is_empty() {
            let next_index = pending
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)))
                .unwrap()
                .0;
            let (part_seed, seed_domain, _) = pending.swap_remove(next_index);
            let current_part = &self.triangles[part_seed].part_id;
            if !expanded_components.insert(self.components[part_seed]) {
                continue;
            }
            let mut queue = VecDeque::from([(part_seed, seed_domain)]);
            visited[part_seed] = true;
            let mut exits: HashMap<usize, (usize, [DVec2; 3], f64)> = HashMap::new();
            while let Some((index, domain)) = queue.pop_front() {
                let original = &self.triangles[index];
                result.extend(coverage_in_domain(original, frame.map(domain), rect));
                for link in &self.links[index] {
                    let target = &self.triangles[link.target];
                    let cross_part = target.part_id != *current_part;
                    if visited[link.target]
                        || (cross_part
                            && (!link.explicit
                                || !adjacent
                                || target.part_id == part
                                || expanded_components.contains(&self.components[link.target])))
                    {
                        continue;
                    }
                    let Some(next) = self.transport(index, domain, link) else {
                        continue;
                    };
                    let min = next[0].min(next[1]).min(next[2]);
                    let max = next[0].max(next[1]).max(next[2]);
                    if physical_center.distance(physical_center.clamp(min, max)) > radius + 0.01 {
                        continue;
                    }
                    if cross_part {
                        let midpoint = (domain[link.edge] + domain[(link.edge + 1) % 3]) * 0.5;
                        let distance = physical_center.distance(midpoint);
                        let candidate = (link.target, next, distance);
                        let entry = exits
                            .entry(self.components[link.target])
                            .or_insert(candidate);
                        if distance
                            .total_cmp(&entry.2)
                            .then(link.target.cmp(&entry.0))
                            .is_lt()
                        {
                            *entry = candidate;
                        }
                    } else {
                        visited[link.target] = true;
                        queue.push_back((link.target, next));
                    }
                }
            }
            pending.extend(exits.into_values());
        }
        result
    }

    fn transport(
        &self,
        source_index: usize,
        domain: [DVec2; 3],
        link: &Link,
    ) -> Option<[DVec2; 3]> {
        let a = domain[link.edge];
        let b = domain[(link.edge + 1) % 3];
        let direction = (b - a).try_normalize()?;
        let perpendicular = DVec2::new(-direction.y, direction.x);
        let side = (domain[(link.edge + 2) % 3] - a)
            .dot(perpendicular)
            .signum();
        let target = &self.triangles[link.target];
        let te = link.target_edge;
        let world = world_points(target);
        let edge = world[(te + 1) % 3] - world[te];
        let edge_length = edge.length();
        if edge_length <= 0. {
            return None;
        }
        let (s, e) = if link.explicit {
            // Authored seam edges may have different lengths and tangential offsets.
            // Place both target endpoints from real geometry, then retain the exact
            // target metric instead of stretching it to the source edge length.
            let source = world_points(&self.triangles[source_index]);
            let origin = source[link.edge];
            let tangent = (source[(link.edge + 1) % 3] - origin).try_normalize()?;
            let to_interior = source[(link.edge + 2) % 3] - origin;
            let inward = (to_interior - tangent * to_interior.dot(tangent)).try_normalize()?;
            let project = |p: DVec3| {
                let offset = (p - origin) * 1000.;
                a + direction * offset.dot(tangent) + perpendicular * side * offset.dot(inward)
            };
            let start = project(world[te]);
            let end = project(world[(te + 1) % 3]);
            let target_direction = (end - start).try_normalize()?;
            let midpoint = (start + end) * 0.5;
            let half = target_direction * edge_length * 500.;
            (midpoint - half, midpoint + half)
        } else if link.reversed {
            (b, a)
        } else {
            (a, b)
        };
        let td = (e - s).try_normalize()?;
        let tn = DVec2::new(-td.y, td.x);
        let to_third = world[(te + 2) % 3] - world[te];
        let x = to_third.dot(edge / edge_length) * 1000.;
        let height = edge.cross(to_third).length() / edge_length * 1000.;
        // Explicit seams pair sampled edges, not necessarily manifold
        // half-edges. Their endpoint order controls correspondence only;
        // using it as triangle handedness can reflect an entire neighbor.
        // Preserve the orientation of the complete exterior sheets instead.
        let target_side = if link.explicit {
            side * self.exterior_winding[source_index] * self.exterior_winding[link.target]
        } else if link.reversed {
            side
        } else {
            -side
        };
        let c = s + td * x + tn * height * target_side;
        let mut next = [DVec2::ZERO; 3];
        next[te] = s;
        next[(te + 1) % 3] = e;
        next[(te + 2) % 3] = c;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgeRef, TriangleId};
    fn triangle(n: u32, part: &str, world: [[f32; 3]; 3], uv: [[f32; 2]; 3]) -> PaintTriangle {
        PaintTriangle {
            id: TriangleId {
                source_mesh: "fixture".into(),
                primitive: 0,
                source_triangle: n,
                fragment: 0,
            },
            part_id: part.into(),
            region_id: "outer".into(),
            chart_id: format!("chart-{n}"),
            world,
            paint_uv: uv,
            source_barycentric: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        }
    }
    fn fixture() -> Vec<PaintTriangle> {
        vec![
            triangle(
                0,
                "door",
                [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                [[0., 0.], [1000., 0.], [0., 1000.]],
            ),
            // Same continuous surface, but independently rotated and translated UV island.
            triangle(
                1,
                "door",
                [[1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
                [[6000., 5000.], [6000., 6000.], [5000., 6000.]],
            ),
            triangle(
                2,
                "spoiler",
                [[0., 0., 0.01], [1., 0., 0.01], [0., 1., 0.01]],
                [[0., 0.], [1000., 0.], [0., 1000.]],
            ),
        ]
    }
    #[test]
    fn computational_uv_cut_keeps_source_image_continuous_and_isolated_parts_excluded() {
        let topology = Topology::new(fixture(), &[]);
        let rect = CoverageRect {
            center: Vec2::splat(450.),
            size: Vec2::splat(800.),
            rotation_rad: 0.2,
        };
        let fragments = topology.coverage("door", rect, true);
        assert!(fragments.iter().any(|f| f.triangle.source_triangle == 1));
        assert!(fragments.iter().all(|f| f.part_id == "door"));
        for fragment in &fragments {
            for (w, image) in fragment.world.iter().zip(&fragment.image_uv) {
                let expected = glam::Mat2::from_angle(-rect.rotation_rad)
                    * (Vec2::new(w[0] * 1000., w[1] * 1000.) - rect.center)
                    / rect.size
                    + Vec2::splat(0.5);
                assert!(expected.distance(Vec2::from_array(*image)) < 1e-5);
            }
        }
        assert_eq!(fragments, topology.coverage("door", rect, true));
    }
    #[test]
    fn only_explicit_connection_crosses_parts_and_preserves_real_gap() {
        let mut triangles = fixture();
        triangles[1].part_id = "rear-door".into();
        for p in &mut triangles[1].world {
            p[0] += 0.003;
        }
        let connection = EdgeConnection {
            id: "door-gap".into(),
            from: EdgeRef {
                triangle: triangles[0].id.clone(),
                edge: 1,
            },
            to: EdgeRef {
                triangle: triangles[1].id.clone(),
                edge: 2,
            },
            reversed: true,
            physical_gap_mm: 3.,
            propagation: true,
        };
        let rect = CoverageRect {
            center: Vec2::splat(450.),
            size: Vec2::splat(800.),
            rotation_rad: 0.,
        };
        let isolated = Topology::new(triangles.clone(), &[]);
        assert!(
            isolated
                .coverage("door", rect, true)
                .iter()
                .all(|f| f.part_id == "door")
        );
        let topology = Topology::new(triangles, &[connection]);
        assert!(
            topology
                .coverage("door", rect, true)
                .iter()
                .any(|f| f.part_id == "rear-door")
        );
        assert!(
            topology
                .coverage("door", rect, false)
                .iter()
                .all(|f| f.part_id == "door")
        );
        assert!(
            topology
                .coverage("door", rect, true)
                .iter()
                .all(|f| f.part_id != "spoiler")
        );
    }
    #[test]
    fn narrow_triangles_at_large_chart_coordinates_keep_source_barycentrics() {
        let triangles = vec![
            triangle(
                0,
                "door",
                [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                [
                    [1_000_000., 1_000_000.],
                    [1_001_000., 1_000_000.],
                    [1_000_000., 1_001_000.],
                ],
            ),
            triangle(
                1,
                "door",
                [[1., 0., 0.], [0., 1., 0.], [0.500001, 0.500001, 0.]],
                [[0., 0.], [1000., 0.], [500., 0.001]],
            ),
        ];
        let topology = Topology::new(triangles, &[]);
        let coverage =
            topology.coverage_at("door", [1_000_450., 1_000_450.], [500., 500.], 0., true);
        let narrow: Vec<_> = coverage
            .iter()
            .filter(|f| f.triangle.source_triangle == 1)
            .collect();
        assert!(
            !narrow.is_empty(),
            "valid narrow source triangles must not be discarded"
        );
        assert!(
            narrow
                .iter()
                .flat_map(|f| &f.paint_uv)
                .any(|p| p[1] > 0.0009)
        );
        for fragment in coverage {
            assert!(
                fragment
                    .world
                    .iter()
                    .flatten()
                    .chain(fragment.paint_uv.iter().flatten())
                    .chain(fragment.image_uv.iter().flatten())
                    .all(|v| v.is_finite())
            );
        }
    }

    #[test]
    fn empty_chart_space_is_not_snapped_to_nearest_surface() {
        let topology = Topology::new(fixture(), &[]);
        assert!(
            topology
                .coverage(
                    "door",
                    CoverageRect {
                        center: Vec2::splat(2500.),
                        size: Vec2::splat(100.),
                        rotation_rad: 0.
                    },
                    true
                )
                .is_empty()
        );
    }
}
