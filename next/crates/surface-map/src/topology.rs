//! Local metric unfolding over surface adjacency. Screen layout is never input.
use crate::{CoverageFragment, CoverageRect, EdgeConnection, PaintTriangle, barycentric_2d};
use glam::{DMat2, DMat3, DVec2, DVec3, Vec2, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy)]
struct Link {
    target: usize,
    edge: usize,
    target_edge: usize,
    reversed: bool,
    explicit: bool,
    source_plane: bool,
    source_hinge: Option<usize>,
}

struct HingeLink {
    world: [DVec3; 2],
    source_third: DVec3,
    target_third: DVec3,
    support: Vec<[f64; 2]>,
}

pub struct Topology {
    pub triangles: Vec<PaintTriangle>,
    links: Vec<Vec<Link>>,
    components: Vec<usize>,
    // Multiplier relating each triangle's vertex winding to its component's
    // consistently oriented exterior sheet. Endpoint order is not sheet order.
    exterior_winding: Vec<f64>,
    // A cut sliver can lose its plane at vehicle-scale f32 coordinates. Recover
    // its metric from a stable fragment of the identical original source face.
    metric_world: Vec<[DVec3; 3]>,
    chart_references: Vec<usize>,
    plane_coordinates: Vec<Option<[DVec2; 3]>>,
    plane_references: Vec<Option<usize>>,
    hinges: Vec<HingeLink>,
}
type Position = [i64; 3];
type EdgeKey = (String, Position, Position);
type SourceEdgeKey = (String, String, u32, u32, [u64; 2], [u64; 2]);
type EdgeEntry = (usize, usize, bool);

// The chart locates the anchor and provides image orientation only. Its local
// stretch/shear is a property of the packed 2D map, never of artwork dimensions:
// all coverage and seam transport use actual physical millimetres.
#[derive(Clone, Copy)]
struct ChartFrame {
    basis: DMat2,
    physical_origin: DVec2,
    chart_origin: DVec2,
}

impl ChartFrame {
    fn new(physical: [DVec2; 3], chart: [DVec2; 3], center: DVec2) -> Self {
        let chart_basis = DMat2::from_cols(chart[1] - chart[0], chart[2] - chart[0])
            * DMat2::from_cols(physical[1] - physical[0], physical[2] - physical[0]).inverse();
        Self::from_jacobian(
            chart_basis,
            physical[0] + chart_basis.inverse() * (center - chart[0]),
            center,
        )
    }

    fn from_jacobian(chart_basis: DMat2, physical_origin: DVec2, chart_origin: DVec2) -> Self {
        // The orthogonal polar factor is the nearest rotation/reflection to the
        // chart Jacobian. Using the full Jacobian would magnify a small image by
        // orders of magnitude on a narrow or distorted chart triangle.
        let reflected = chart_basis.determinant() < 0.;
        let x = if reflected {
            DVec2::new(
                chart_basis.x_axis.x - chart_basis.y_axis.y,
                chart_basis.x_axis.y + chart_basis.y_axis.x,
            )
        } else {
            DVec2::new(
                chart_basis.x_axis.x + chart_basis.y_axis.y,
                chart_basis.x_axis.y - chart_basis.y_axis.x,
            )
        }
        .normalize();
        let y = DVec2::new(-x.y, x.x) * if reflected { -1. } else { 1. };
        Self {
            basis: DMat2::from_cols(x, y),
            physical_origin,
            chart_origin,
        }
    }

    fn map(&self, points: [DVec2; 3]) -> [DVec2; 3] {
        points.map(|p| self.chart_origin + self.basis * (p - self.physical_origin))
    }
}

fn world_points(triangle: &PaintTriangle) -> [DVec3; 3] {
    triangle.world.map(|p| DVec3::from_array(p.map(f64::from)))
}

fn physical_triangle(triangle: &PaintTriangle) -> [DVec2; 3] {
    physical_points(world_points(triangle))
}

fn physical_points([a, b, c]: [DVec3; 3]) -> [DVec2; 3] {
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

fn plane_isometry(source: [DVec2; 3], domain: [DVec2; 3]) -> Option<(DMat2, DVec2)> {
    let e = (0..3).max_by(|&a, &b| {
        (source[(a + 1) % 3] - source[a])
            .length_squared()
            .total_cmp(&(source[(b + 1) % 3] - source[b]).length_squared())
    })?;
    let x = (source[(e + 1) % 3] - source[e]).try_normalize()?;
    let y = DVec2::new(-x.y, x.x);
    let dx = (domain[(e + 1) % 3] - domain[e]).try_normalize()?;
    let source_sign = (source[1] - source[0])
        .perp_dot(source[2] - source[0])
        .signum();
    let domain_sign = (domain[1] - domain[0])
        .perp_dot(domain[2] - domain[0])
        .signum();
    let dy = DVec2::new(-dx.y, dx.x) * source_sign * domain_sign;
    let rotation = DMat2::from_cols(dx, dy) * DMat2::from_cols(x, y).transpose();
    Some((rotation, domain[e] - rotation * source[e]))
}

fn segment_intersects_rect(mut a: DVec2, mut b: DVec2, half: DVec2) -> bool {
    for axis in 0..2 {
        for sign in [-1., 1.] {
            let da = half[axis] - sign * a[axis];
            let db = half[axis] - sign * b[axis];
            if da < 0. && db < 0. {
                return false;
            }
            if (da < 0.) != (db < 0.) {
                let intersection = a.lerp(b, da / (da - db));
                if da < 0. {
                    a = intersection;
                } else {
                    b = intersection;
                }
            }
        }
    }
    true
}

fn tangent_axes(triangle: &PaintTriangle, basis: DMat2) -> (DVec3, DVec3) {
    world_axes(world_points(triangle), basis)
}

fn world_axes(w: [DVec3; 3], basis: DMat2) -> (DVec3, DVec3) {
    let x = (w[1] - w[0]).normalize();
    let to_third = w[2] - w[0];
    let y = (to_third - x * to_third.dot(x)).normalize();
    (
        x * basis.x_axis.x + y * basis.y_axis.x,
        x * basis.x_axis.y + y * basis.y_axis.y,
    )
}

/// Orthogonal physical-mm axes at an identified fragment, without a UV search.
/// Used when migrating a packed chart while preserving the source-face anchor.
pub fn triangle_tangent_axes(triangle: &PaintTriangle) -> Option<(Vec3, Vec3)> {
    let chart = triangle.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
    let frame = ChartFrame::new(physical_triangle(triangle), chart, chart[0]);
    let (u, v) = tangent_axes(triangle, frame.basis);
    (u.is_finite() && v.is_finite()).then_some((u.as_vec3(), v.as_vec3()))
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

fn normalized_source(t: &PaintTriangle) -> [DVec2; 3] {
    t.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        DVec2::new(b.y, b.z) / b.element_sum()
    })
}

// Retain the homogeneous barycentric determinant: subtracting two normalized
// coordinates can cancel the positive width of an original-edge micro mask.
fn source_winding(t: &PaintTriangle) -> f64 {
    let b = t.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        b / b.element_sum()
    });
    let det = DMat3::from_cols(b[0], b[1], b[2]).determinant();
    if det == 0. || !det.is_finite() {
        0.
    } else {
        det.signum()
    }
}

fn minimum_height(w: [DVec3; 3]) -> f64 {
    let longest = (0..3)
        .map(|i| (w[(i + 1) % 3] - w[i]).length())
        .fold(0., f64::max);
    (w[1] - w[0]).cross(w[2] - w[0]).length() / longest
}

fn stable_metrics(triangles: &[PaintTriangle]) -> (Vec<[DVec3; 3]>, Vec<usize>) {
    let mut sources = HashMap::new();
    let mut charts = HashMap::new();
    for (i, t) in triangles.iter().enumerate() {
        let b = normalized_source(t);
        let score = (b[1] - b[0]).perp_dot(b[2] - b[0]).abs();
        if !score.is_finite() || score <= 1e-12 || minimum_height(world_points(t)) <= 2e-6 {
            continue;
        }
        let source = (
            t.id.source_mesh.as_str(),
            t.id.primitive,
            t.id.source_triangle,
        );
        let chart = (source, t.part_id.as_str(), t.chart_id.as_str());
        for entry in [
            sources.entry(source).or_insert((score, i)),
            charts.entry(chart).or_insert((score, i)),
        ] {
            if score > entry.0 {
                *entry = (score, i);
            }
        }
    }
    let mut worlds = Vec::with_capacity(triangles.len());
    let mut references = Vec::with_capacity(triangles.len());
    for (i, t) in triangles.iter().enumerate() {
        let raw = world_points(t);
        let source = (
            t.id.source_mesh.as_str(),
            t.id.primitive,
            t.id.source_triangle,
        );
        let mut world = raw;
        let mut reference = i;
        if let Some(&(_, sibling)) = sources.get(&source) {
            let template = &triangles[sibling];
            let b = normalized_source(template);
            let inverse = DMat2::from_cols(b[1] - b[0], b[2] - b[0]).inverse();
            let w = world_points(template);
            let restored = normalized_source(t).map(|p| {
                let q = inverse * (p - b[0]);
                w[0] + (w[1] - w[0]) * q.x + (w[2] - w[0]) * q.y
            });
            // Source identity is authoritative, but an inconsistent reference
            // must never move an unrelated sheet into this metric frame.
            if restored
                .iter()
                .zip(raw)
                .all(|(a, b)| a.is_finite() && a.distance(b) <= 5e-6)
                && source_winding(t) != 0.
            {
                world = restored;
                reference = charts
                    .get(&(source, t.part_id.as_str(), t.chart_id.as_str()))
                    .map_or(i, |&(_, index)| index);
            }
        }
        worlds.push(world);
        references.push(reference);
    }
    (worlds, references)
}

// A clipped mask can contain several disjoint fragments of one original GLB
// triangle. Its affine plane stays continuous even where the paint mask has a
// hole. Grouping extends artwork only to retained mask triangles; it never
// creates paint in that hole or joins a different original face/part.
fn source_planes(
    triangles: &[PaintTriangle],
    worlds: &mut [[DVec3; 3]],
) -> (Vec<Option<[DVec2; 3]>>, Vec<Option<usize>>) {
    let mut groups: HashMap<_, Vec<usize>> = HashMap::new();
    for (i, t) in triangles.iter().enumerate() {
        groups
            .entry((
                t.id.source_mesh.as_str(),
                t.id.primitive,
                t.id.source_triangle,
                t.part_id.as_str(),
            ))
            .or_default()
            .push(i);
    }
    let mut coordinates = vec![None; triangles.len()];
    let mut references = vec![None; triangles.len()];
    for members in groups.values() {
        let reference = members
            .iter()
            .copied()
            .filter(|&i| minimum_height(worlds[i]) > 2e-6)
            .max_by(|&a, &b| {
                let area = |i| {
                    let s = normalized_source(&triangles[i]);
                    (s[1] - s[0]).perp_dot(s[2] - s[0]).abs()
                };
                area(a).total_cmp(&area(b))
            });
        let Some(reference) = reference else { continue };
        let s = normalized_source(&triangles[reference]);
        let source = DMat2::from_cols(s[1] - s[0], s[2] - s[0]);
        if source.determinant().abs() <= 1e-12 {
            continue;
        }
        let w = worlds[reference];
        let p = physical_points(w);
        let physical = DMat2::from_cols(p[1] - p[0], p[2] - p[0]) * source.inverse();
        let (x, y) = world_axes(w, DMat2::IDENTITY);
        for &i in members {
            let q = normalized_source(&triangles[i])
                .map(|s| physical * (s - normalized_source(&triangles[reference])[0]));
            let restored = q.map(|p| w[0] + (x * p.x + y * p.y) * 0.001);
            if q.iter().all(|p| p.is_finite())
                && source_winding(&triangles[i]) != 0.
                && restored
                    .iter()
                    .zip(world_points(&triangles[i]))
                    .all(|(a, b)| a.is_finite() && a.distance(b) <= 5e-6)
            {
                coordinates[i] = Some(q);
                references[i] = Some(reference);
                worlds[i] = restored;
            }
        }
    }
    (coordinates, references)
}

impl Topology {
    pub fn new(triangles: Vec<PaintTriangle>, connections: &[EdgeConnection]) -> Self {
        let (mut metric_world, chart_references) = stable_metrics(&triangles);
        let (plane_coordinates, plane_references) = source_planes(&triangles, &mut metric_world);
        let mut links = vec![vec![]; triangles.len()];
        let mut hinges = vec![];
        let mut edges: HashMap<EdgeKey, Vec<EdgeEntry>> = HashMap::new();
        let mut source_edges: HashMap<SourceEdgeKey, Vec<EdgeEntry>> = HashMap::new();
        for (i, t) in triangles.iter().enumerate() {
            let source = normalized_source(t).map(|b| b.to_array().map(f64::to_bits));
            for edge in 0..3 {
                let a = position(t.world[edge]);
                let b = position(t.world[(edge + 1) % 3]);
                let forward = a <= b;
                let key = (t.part_id.clone(), a.min(b), a.max(b));
                edges.entry(key).or_default().push((i, edge, forward));
                let a = source[edge];
                let b = source[(edge + 1) % 3];
                let key = (
                    t.part_id.clone(),
                    t.id.source_mesh.clone(),
                    t.id.primitive,
                    t.id.source_triangle,
                    a.min(b),
                    a.max(b),
                );
                source_edges.entry(key).or_default().push((i, edge, a <= b));
            }
        }
        // Within one original face, source barycentrics define real shared
        // edges. A spatial hash can weld unrelated submicrometre edges and
        // rotate an entire artwork through the false adjacency.
        for entries in source_edges.values() {
            if let [(a, ae, af), (b, be, bf)] = entries.as_slice() {
                links[*a].push(Link {
                    target: *b,
                    edge: *ae,
                    target_edge: *be,
                    reversed: af != bf,
                    explicit: false,
                    source_plane: false,
                    source_hinge: None,
                });
                links[*b].push(Link {
                    target: *a,
                    edge: *be,
                    target_edge: *ae,
                    reversed: af != bf,
                    explicit: false,
                    source_plane: false,
                    source_hinge: None,
                });
            }
        }
        for entries in edges.values() {
            // Non-manifold edges require explicit authoring; never choose an arbitrary sheet.
            if let [(a, ae, af), (b, be, bf)] = entries.as_slice() {
                let at = &triangles[*a];
                let bt = &triangles[*b];
                if at.id.source_mesh == bt.id.source_mesh
                    && at.id.primitive == bt.id.primitive
                    && at.id.source_triangle == bt.id.source_triangle
                {
                    continue;
                }
                // The quantized key only finds an existing common edge. Its
                // endpoints may fall into the same micrometre cell, so its
                // lexicographic order cannot determine real correspondence.
                let aw = metric_world[*a];
                let bw = metric_world[*b];
                let forward_error = aw[*ae].distance_squared(bw[*be])
                    + aw[(*ae + 1) % 3].distance_squared(bw[(*be + 1) % 3]);
                let reverse_error = aw[*ae].distance_squared(bw[(*be + 1) % 3])
                    + aw[(*ae + 1) % 3].distance_squared(bw[*be]);
                let reversed = if forward_error == reverse_error {
                    af != bf
                } else {
                    reverse_error < forward_error
                };
                links[*a].push(Link {
                    target: *b,
                    edge: *ae,
                    target_edge: *be,
                    reversed,
                    explicit: false,
                    source_plane: false,
                    source_hinge: None,
                });
                links[*b].push(Link {
                    target: *a,
                    edge: *be,
                    target_edge: *ae,
                    reversed,
                    explicit: false,
                    source_plane: false,
                    source_hinge: None,
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
            if let Some(h) = &c.source_hinge {
                let Some((original_a, original_b)) =
                    crate::source_hinge_geometry(c, &triangles[a], &triangles[b])
                else {
                    continue;
                };
                let sign = |i: usize| {
                    let s = normalized_source(&triangles[i]);
                    (s[1] - s[0]).perp_dot(s[2] - s[0]).signum()
                };
                let reversed = (if c.reversed { 1. } else { -1. }) * sign(a) * sign(b) > 0.;
                let third_a = original_a[(h.from_source_edge as usize + 2) % 3];
                let third_b = original_b[(h.to_source_edge as usize + 2) % 3];
                let world = h.world.map(|p| DVec3::from_array(p.map(f64::from)));
                for (source, target, source_third, target_third) in
                    [(a, b, third_a, third_b), (b, a, third_b, third_a)]
                {
                    let hinge = hinges.len();
                    hinges.push(HingeLink {
                        world,
                        source_third,
                        target_third,
                        support: h
                            .support_intervals
                            .iter()
                            .map(|p| p.map(f64::from))
                            .collect(),
                    });
                    links[source].push(Link {
                        target,
                        edge: 0,
                        target_edge: 0,
                        reversed,
                        explicit: false,
                        source_plane: false,
                        source_hinge: Some(hinge),
                    });
                }
                continue;
            }
            links[a].push(Link {
                target: b,
                edge: c.from.edge as usize,
                target_edge: c.to.edge as usize,
                reversed: c.reversed,
                explicit: true,
                source_plane: false,
                source_hinge: None,
            });
            links[b].push(Link {
                target: a,
                edge: c.to.edge as usize,
                target_edge: c.from.edge as usize,
                reversed: c.reversed,
                explicit: true,
                source_plane: false,
                source_hinge: None,
            });
        }
        for (i, reference) in plane_references.iter().copied().enumerate() {
            let Some(reference) = reference.filter(|&r| r != i) else {
                continue;
            };
            // Here correspondence means consistent source-plane winding,
            // independent of fragment edge order or packed-chart reflection.
            let reversed = source_winding(&triangles[i]) == source_winding(&triangles[reference]);
            links[i].push(Link {
                target: reference,
                edge: 0,
                target_edge: 0,
                reversed,
                explicit: false,
                source_plane: true,
                source_hinge: None,
            });
            links[reference].push(Link {
                target: i,
                edge: 0,
                target_edge: 0,
                reversed,
                explicit: false,
                source_plane: true,
                source_hinge: None,
            });
        }
        for neighbors in &mut links {
            neighbors.sort_by_key(|l| (!l.source_plane, l.edge, l.target, l.target_edge));
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
                let [a, b, c] = metric_world[index];
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
            metric_world,
            chart_references,
            plane_coordinates,
            plane_references,
            hinges,
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
        let frame = |i: usize| {
            let reference = self.frame_reference(i);
            // Unit image directions use the same orthogonal chart frame as
            // coverage. Raw inverse UV derivatives would add chart shear to
            // the orientation every time a drag crosses a surface boundary.
            let basis =
                ChartFrame::from_jacobian(self.chart_jacobian(i), DVec2::ZERO, DVec2::ZERO).basis;
            let (u, v) = world_axes(self.metric_world[reference], basis);
            let w = self.metric_world[reference];
            let n = ((w[1] - w[0]).cross(w[2] - w[0]) * self.exterior_winding[reference])
                .normalize()
                .as_vec3();
            (u.as_vec3(), v.as_vec3(), n)
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

    fn chart_jacobian(&self, index: usize) -> DMat2 {
        let chart_reference = self.chart_references[index];
        let reference = self.frame_reference(index);
        let w = self.metric_world[reference];
        let physical = physical_points(w);
        let chart = self.triangles[chart_reference]
            .paint_uv
            .map(|p| Vec2::from_array(p).as_dvec2());
        if chart_reference != reference {
            // A separate packed micro chart can lack a stable triangle of its
            // own. Derive its gradient in homogeneous source barycentrics,
            // retaining widths that cancel in world-coordinate subtraction.
            let b = self.triangles[chart_reference].source_barycentric.map(|b| {
                let b = DVec3::from_array(b.map(f64::from));
                b / b.element_sum()
            });
            let chart_from_source = DMat3::from_cols(
                DVec3::new(chart[0].x, chart[0].y, 1.),
                DVec3::new(chart[1].x, chart[1].y, 1.),
                DVec3::new(chart[2].x, chart[2].y, 1.),
            ) * DMat3::from_cols(b[0], b[1], b[2]).inverse();
            let source = normalized_source(&self.triangles[reference]);
            let source_to_physical =
                DMat2::from_cols(physical[1] - physical[0], physical[2] - physical[0])
                    * DMat2::from_cols(source[1] - source[0], source[2] - source[0]).inverse();
            return DMat2::from_cols(
                (chart_from_source.y_axis - chart_from_source.x_axis).truncate(),
                (chart_from_source.z_axis - chart_from_source.x_axis).truncate(),
            ) * source_to_physical.inverse();
        }
        DMat2::from_cols(chart[1] - chart[0], chart[2] - chart[0])
            * DMat2::from_cols(physical[1] - physical[0], physical[2] - physical[0]).inverse()
    }

    fn frame_reference(&self, index: usize) -> usize {
        let reference = self.chart_references[index];
        if self.plane_coordinates[index].is_some() && self.plane_coordinates[reference].is_some() {
            if minimum_height(self.metric_world[reference]) > 2e-6 {
                reference
            } else {
                self.plane_references[index].unwrap_or(reference)
            }
        } else {
            index
        }
    }

    fn anchor_chart_frame(&self, index: usize, center: DVec2, bary: Vec3) -> ChartFrame {
        let reference = self.frame_reference(index);
        let w = self.metric_world[reference];
        let (x, y) = world_axes(w, DMat2::IDENTITY);
        let physical = self.metric_world[index].map(|p| {
            let delta = (p - w[0]) * 1000.;
            DVec2::new(delta.dot(x), delta.dot(y))
        });
        let bary = bary.as_dvec3();
        let origin = physical[0] * bary.x + physical[1] * bary.y + physical[2] * bary.z;
        ChartFrame::from_jacobian(self.chart_jacobian(index), origin, center)
    }

    pub(crate) fn fragment_tangent_axes(&self, index: usize) -> Option<(Vec3, Vec3)> {
        self.metric_world.get(index)?;
        let world = self.metric_world[self.frame_reference(index)];
        let basis =
            ChartFrame::from_jacobian(self.chart_jacobian(index), DVec2::ZERO, DVec2::ZERO).basis;
        let (u, v) = world_axes(world, basis);
        (u.is_finite() && v.is_finite()).then_some((u.as_vec3(), v.as_vec3()))
    }

    /// World origin and orthogonal image X/Y vectors, each one physical mm.
    /// Packed-chart stretch/shear does not affect these vectors or coverage.
    pub fn tangent_frame(&self, part: &str, point: Vec2) -> Option<(Vec3, Vec3, Vec3)> {
        let (index, bary) = self.locate(part, point)?;
        let frame = self.anchor_chart_frame(index, point.as_dvec2(), bary);
        let w = self.metric_world[self.frame_reference(index)];
        let local_x = (w[1] - w[0]).normalize();
        let to_third = w[2] - w[0];
        let local_y = (to_third - local_x * to_third.dot(local_x)).normalize();
        let origin =
            w[0] + (local_x * frame.physical_origin.x + local_y * frame.physical_origin.y) * 0.001;
        let (u, v) = world_axes(w, frame.basis);
        (origin.is_finite() && u.is_finite() && v.is_finite()).then_some((
            origin.as_vec3(),
            (u * 0.001).as_vec3(),
            (v * 0.001).as_vec3(),
        ))
    }

    /// Apply a local physical-mm offset in the orthogonal image X/Y frame.
    /// Returns the translated chart point (point + delta), not the delta alone.
    pub fn chart_offset(&self, part: &str, point: Vec2, offset_metric_mm: Vec2) -> Option<Vec2> {
        if !offset_metric_mm.is_finite() {
            return None;
        }
        let (index, bary) = self.locate(part, point)?;
        let frame = self.anchor_chart_frame(index, point.as_dvec2(), bary);
        let chart_basis = self.chart_jacobian(index);
        let translated =
            point.as_dvec2() + chart_basis * frame.basis.transpose() * offset_metric_mm.as_dvec2();
        translated.is_finite().then_some(translated.as_vec2())
    }

    /// Transport across retained source-plane masks, real triangle edges and
    /// authored seams. Every output remains clipped to its retained mask.
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
        let Some((seed, bary)) = self.locate(part, rect.center) else {
            return vec![];
        };
        let frame = self.anchor_chart_frame(seed, rect.center.as_dvec2(), bary);
        // Begin on the same chart's stable, identical source plane. Rebuilding
        // axes from a nearly collinear micro triangle loses its source normal.
        // The original micro mask is still visited and clipped through its
        // source-plane link; no retained mask is enlarged or removed.
        let seed = self.frame_reference(seed);
        let seed_domain = physical_points(self.metric_world[seed]);
        let physical_center = frame.physical_origin;
        let radius = rect.size.as_dvec2().length() * 0.5;
        if !physical_center.is_finite()
            || !frame.basis.is_finite()
            || !radius.is_finite()
            || radius <= 0.
        {
            return vec![];
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
                    if let Some(hinge_index) = link.source_hinge {
                        let hinge = &self.hinges[hinge_index];
                        let Some([a, b, _]) = self.hinge_domain(index, domain, hinge) else {
                            continue;
                        };
                        let image_rotation =
                            DMat2::from_angle(-f64::from(rect.rotation_rad)) * frame.basis;
                        if !hinge.support.iter().any(|&[start, end]| {
                            segment_intersects_rect(
                                image_rotation * (a.lerp(b, start) - physical_center),
                                image_rotation * (a.lerp(b, end) - physical_center),
                                rect.size.as_dvec2() * 0.5,
                            )
                        }) {
                            continue;
                        }
                    }
                    let Some(next) = self.transport(index, domain, link) else {
                        continue;
                    };
                    let min = next[0].min(next[1]).min(next[2]);
                    let max = next[0].max(next[1]).max(next[2]);
                    // A plane's stable reference can lie beyond the image even
                    // when another retained member is close to the anchor.
                    // Visit that reference to reach all masks on the plane;
                    // clipping still emits nothing outside the image/mask.
                    if !link.source_plane
                        && link.source_hinge.is_none()
                        && physical_center.distance(physical_center.clamp(min, max)) > radius + 0.01
                    {
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
        if let Some(hinge_index) = link.source_hinge {
            let hinge = &self.hinges[hinge_index];
            let [a, b, source_third] = self.hinge_domain(source_index, domain, hinge)?;
            let direction = (b - a).try_normalize()?;
            let perpendicular = DVec2::new(-direction.y, direction.x);
            let side = (source_third - a).dot(perpendicular).signum();
            let world_direction = (hinge.world[1] - hinge.world[0]).try_normalize()?;
            let to_interior = hinge.target_third - hinge.world[0];
            let inward = (to_interior - world_direction * to_interior.dot(world_direction))
                .try_normalize()?;
            return Some(self.metric_world[link.target].map(|p| {
                let q = (p - hinge.world[0]) * 1000.;
                a + direction * q.dot(world_direction) - perpendicular * side * q.dot(inward)
            }));
        }
        if link.source_plane {
            let source = self.plane_coordinates[source_index]?;
            let target = self.plane_coordinates[link.target]?;
            // An isometry from the longest source edge avoids inversion of a
            // microscopic fragment's nearly singular barycentric matrix.
            let (rotation, offset) = plane_isometry(source, domain)?;
            return Some(target.map(|p| offset + rotation * p));
        }
        let a = domain[link.edge];
        let b = domain[(link.edge + 1) % 3];
        let direction = (b - a).try_normalize()?;
        let perpendicular = DVec2::new(-direction.y, direction.x);
        let side = (domain[(link.edge + 2) % 3] - a)
            .dot(perpendicular)
            .signum();
        let te = link.target_edge;
        let world = self.metric_world[link.target];
        let edge = world[(te + 1) % 3] - world[te];
        let edge_length = edge.length();
        if edge_length <= 0. {
            return None;
        }
        let (s, e) = if link.explicit {
            // Authored seam edges may have different lengths and tangential offsets.
            // Place both target endpoints from real geometry, then retain the exact
            // target metric instead of stretching it to the source edge length.
            let source = self.metric_world[source_index];
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

    fn hinge_domain(
        &self,
        index: usize,
        domain: [DVec2; 3],
        hinge: &HingeLink,
    ) -> Option<[DVec2; 3]> {
        let w = self.metric_world[index];
        let (rotation, offset) = plane_isometry(physical_points(w), domain)?;
        let (x, y) = world_axes(w, DMat2::IDENTITY);
        Some(
            [hinge.world[0], hinge.world[1], hinge.source_third].map(|p| {
                let q = (p - w[0]) * 1000.;
                offset + rotation * DVec2::new(q.dot(x), q.dot(y))
            }),
        )
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
    fn submicrometre_spatial_collision_cannot_weld_distinct_source_edges() {
        let p = [0.25, 0.25];
        let q = [0.25000024, 0.25];
        let r = [0.1, 0.1];
        let make = |fragment: u64, points: [[f32; 2]; 3]| {
            let mut t = triangle(
                7,
                "door",
                points.map(|p| [p[0], p[1], 0.]),
                points.map(|p| [p[0] * 1000., p[1] * 1000.]),
            );
            t.id.fragment = fragment;
            t.chart_id = "same-source-chart".into();
            t.source_barycentric = points.map(|p| [1. - p[0] - p[1], p[0], p[1]]);
            t
        };
        let a = make(0, [p, q, r]);
        let b = make(1, [[0.25, 0.25000012], [0.25, 0.25000036], [0.4, 0.1]]);
        let c = make(2, [r, q, [0.5, 0.]]);
        // These perpendicular short edges falsely share the old spatial key.
        // Their exact source endpoints are different, while A/C share a real
        // edge even though the collapsed spatial cell makes it non-manifold.
        assert_eq!(position(a.world[0]), position(a.world[1]));
        assert_eq!(position(a.world[0]), position(b.world[0]));
        assert_eq!(position(a.world[0]), position(b.world[1]));
        let topology = Topology::new(vec![a, b, c], &[]);
        let physical_links = |i: usize| {
            topology.links[i]
                .iter()
                .filter(|link| !link.source_plane)
                .map(|link| link.target)
                .collect::<Vec<_>>()
        };
        assert_eq!(physical_links(0), vec![2]);
        assert!(physical_links(1).is_empty());
        assert_eq!(topology.components[0], topology.components[2]);
        // Separate retained masks on this same original plane can receive the
        // same artwork, without treating their unrelated tiny edges as shared.
        assert_eq!(topology.components[0], topology.components[1]);
        let center = topology.triangles[0]
            .paint_uv
            .map(Vec2::from_array)
            .into_iter()
            .sum::<Vec2>()
            / 3.;
        let (located, _) = topology.locate("door", center).unwrap();
        assert_eq!(located, 0, "regression must start on the tiny fragment");
        let rect = CoverageRect {
            center,
            size: Vec2::splat(1000.),
            rotation_rad: 0.17,
        };
        let coverage = topology.coverage("door", rect, true);
        assert!(coverage.iter().any(|f| f.triangle.fragment == 2));
        assert!(coverage.iter().any(|f| f.triangle.fragment == 1));
        let (origin, _, _) = topology.tangent_frame("door", center).unwrap();
        for f in &coverage {
            for (world, image) in f.world.iter().zip(&f.image_uv) {
                let expected = glam::Mat2::from_angle(-rect.rotation_rad)
                    * (Vec2::new(world[0], world[1]) - origin.truncate())
                    * 1000.
                    / rect.size
                    + Vec2::splat(0.5);
                assert!(
                    expected.distance(Vec2::from_array(*image)) < 1e-5,
                    "a false spatial weld must not rotate the broad same-source image"
                );
            }
        }
    }

    #[test]
    fn chart_stretch_and_shear_do_not_change_artwork_millimetres() {
        let center = DVec2::new(450., 450.);
        // Symmetric positive stretch has no image rotation. Include the extreme
        // anisotropy of a narrow chart triangle, as well as a reflected packing.
        for stretch in [
            DMat2::from_diagonal(DVec2::splat(0.01)),
            DMat2::from_cols(DVec2::new(0.01, 0.12), DVec2::new(0.12, 2.5)),
            DMat2::from_cols(DVec2::new(2., 0.7), DVec2::new(0.7, 0.8)),
        ] {
            for mirror in [1., -1.] {
                let packing =
                    DMat2::from_angle(0.63) * DMat2::from_diagonal(DVec2::new(mirror, 1.));
                let chart = packing * stretch;
                let offset = DVec2::new(4000., 6000.);
                let mut triangles = fixture();
                triangles.truncate(2);
                for triangle in &mut triangles {
                    triangle.paint_uv = triangle.world.map(|w| {
                        (offset + chart * DVec2::new(w[0] as f64, w[1] as f64) * 1000.)
                            .as_vec2()
                            .to_array()
                    });
                }
                let topology = Topology::new(triangles, &[]);
                let rect = CoverageRect {
                    center: (offset + chart * center).as_vec2(),
                    size: Vec2::new(300., 180.),
                    rotation_rad: 0.17,
                };
                let coverage = topology.coverage("door", rect, true);
                assert!(!coverage.is_empty());
                let mut covered_area = 0.;
                for fragment in &coverage {
                    for (world, actual) in fragment.world.iter().zip(&fragment.image_uv) {
                        let physical = DVec2::new(world[0] as f64, world[1] as f64) * 1000.;
                        let expected = DMat2::from_angle(-rect.rotation_rad as f64)
                            * packing
                            * (physical - center)
                            / rect.size.as_dvec2()
                            + DVec2::splat(0.5);
                        assert!(
                            expected.distance(Vec2::from_array(*actual).as_dvec2()) < 0.0002,
                            "chart scale/shear must not stretch a physical-size image"
                        );
                    }
                    let w: Vec<_> = fragment
                        .world
                        .iter()
                        .map(|w| Vec3::from_array(*w))
                        .collect();
                    covered_area += (w[1] - w[0]).cross(w[2] - w[0]).length() as f64 * 500_000.;
                }
                assert!((covered_area - 300. * 180.).abs() < 0.1);
            }
        }
    }

    #[test]
    fn drag_orientation_uses_the_same_stretch_free_frame_as_coverage() {
        let at = Vec2::new(200., 300.);
        let angle = 0.31_f32;
        let vertical = Vec2::new(-angle.sin(), angle.cos());
        for mirror in [1., -1.] {
            let packing =
                glam::Mat2::from_angle(0.63) * glam::Mat2::from_diagonal(Vec2::new(mirror, 1.));
            let stretch = glam::Mat2::from_cols(Vec2::new(2., 0.7), Vec2::new(0.7, 0.8));
            let target_chart = packing * stretch;
            let mut source = fixture().remove(0);
            source.chart_id = "shared-chart".into();
            let mut target = source.clone();
            target.id.source_triangle = 1;
            target.part_id = "target".into();
            target.paint_uv = target
                .paint_uv
                .map(|p| (target_chart * Vec2::from_array(p) + Vec2::splat(2000.)).to_array());
            let topology = Topology::new(vec![source, target], &[]);
            let (result, reflected) = topology
                .transport_orientation(
                    "door",
                    at,
                    "target",
                    target_chart * at + Vec2::splat(2000.),
                    angle,
                )
                .unwrap();
            let carried = packing.inverse() * Vec2::new(-result.sin(), result.cos());
            assert!(carried.distance(vertical) < 1e-5);
            assert_eq!(reflected, mirror < 0.);
        }
    }

    #[test]
    fn physical_tangent_frame_and_chart_offset_agree_under_distorted_packing() {
        let world_center = Vec3::new(0.2, 0.3, 0.);
        for mirror in [1., -1.] {
            let packing =
                glam::Mat2::from_angle(0.63) * glam::Mat2::from_diagonal(Vec2::new(mirror, 1.));
            let stretch = glam::Mat2::from_cols(Vec2::new(2., 0.7), Vec2::new(0.7, 0.8));
            let chart = packing * stretch;
            let offset = Vec2::splat(2000.);
            let mut triangle = fixture().remove(0);
            triangle.paint_uv = triangle
                .paint_uv
                .map(|p| (chart * Vec2::from_array(p) + offset).to_array());
            let topology = Topology::new(vec![triangle], &[]);
            let point = chart * world_center.truncate() * 1000. + offset;
            let (origin, u, v) = topology.tangent_frame("door", point).unwrap();
            assert!(origin.distance(world_center) < 1e-6);
            assert!((u.length() - 0.001).abs() < 1e-9);
            assert!((v.length() - 0.001).abs() < 1e-9);
            assert!(u.dot(v).abs() < 1e-12);
            for metric_offset in [Vec2::new(13., -7.), Vec2::new(-11., 19.)] {
                let translated = topology.chart_offset("door", point, metric_offset).unwrap();
                let (moved, _, _) = topology.tangent_frame("door", translated).unwrap();
                let expected = origin + u * metric_offset.x + v * metric_offset.y;
                assert!(moved.distance(expected) < 1e-6);
                let restored = topology
                    .chart_offset("door", translated, -metric_offset)
                    .unwrap();
                assert!(restored.distance(point) < 0.0003);
            }
        }
        let topology = Topology::new(fixture(), &[]);
        assert!(topology.tangent_frame("missing", Vec2::ZERO).is_none());
        assert!(
            topology
                .chart_offset("door", Vec2::ZERO, Vec2::NAN)
                .is_none()
        );
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
            source_hinge: None,
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
