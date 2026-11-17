//! A replacement may split/repack fragments, but cannot remove reviewed source
//! paint or move saved artwork on the source model.
use glam::{DMat2, DVec2, DVec3, Vec3};
use itasha_surface_map::{
    PaintSurfaceMap, PaintTriangle,
    topology::Topology,
    transition::{MappingTransition, SourcePartCorrection},
};
use std::collections::{BTreeMap, BTreeSet};
mod support;

type SourceKey = (String, u32, u32, String);

// Reviewed source face 32590 is physically at the front-left fender. Its two
// historical rear-bumper fragments duplicated that same painted source face.
// This exact identity is the only allowed classification correction.
const SOURCE_PART_CORRECTIONS: [SourcePartCorrection<'static>; 1] = [SourcePartCorrection {
    source_mesh: "body.glb/node/175/primitive/0",
    primitive: 0,
    source_triangle: 32590,
    from_part: "su7.rear-bumper",
    to_part: "su7.front-left-fender",
}];

fn corrected_part(t: &PaintTriangle) -> &str {
    for correction in &SOURCE_PART_CORRECTIONS {
        if t.id.source_mesh == correction.source_mesh
            && t.id.primitive == correction.primitive
            && t.id.source_triangle == correction.source_triangle
            && t.part_id == correction.from_part
        {
            return correction.to_part;
        }
    }
    &t.part_id
}

fn predecessor() -> PaintSurfaceMap {
    let path = support::paint_root().join("su7-v21-classified-only/paint-map.json");
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    map.validate().unwrap();
    assert_eq!(
        map.paint_fingerprint,
        "b16c24a5ff3ba086c545841dd92fd5d0a99ea50ab0cd270b94caca9d8b767db1"
    );
    map
}

fn source_key(t: &PaintTriangle) -> SourceKey {
    (
        t.id.source_mesh.clone(),
        t.id.primitive,
        t.id.source_triangle,
        t.part_id.clone(),
    )
}

fn source_polygon(t: &PaintTriangle) -> Vec<DVec2> {
    let mut p = t
        .source_barycentric
        .map(|b| DVec2::new(f64::from(b[1]), f64::from(b[2])))
        .to_vec();
    if (p[1] - p[0]).perp_dot(p[2] - p[0]) < 0. {
        p.reverse();
    }
    p
}

fn polygon_area(p: &[DVec2]) -> f64 {
    if p.len() < 3 {
        return 0.;
    }
    (1..p.len() - 1)
        .map(|i| (p[i] - p[0]).perp_dot(p[i + 1] - p[0]))
        .sum::<f64>()
        .abs()
        * 0.5
}

fn half_plane(p: &[DVec2], a: DVec2, b: DVec2, sign: f64) -> Vec<DVec2> {
    let mut out = Vec::new();
    if p.is_empty() {
        return out;
    }
    let signed = |q: DVec2| (b - a).perp_dot(q - a) * sign;
    let mut prior = p[p.len() - 1];
    let mut pd = signed(prior);
    for &q in p {
        let d = signed(q);
        if (pd >= 0.) != (d >= 0.) {
            out.push(prior + (q - prior) * (pd / (pd - d)));
        }
        if d >= 0. {
            out.push(q);
        }
        prior = q;
        pd = d;
    }
    out
}

// Subtract an actual convex source fragment. Summing replacement triangle
// areas would allow duplicated paint to hide a missing portion of the mask.
fn subtract(p: Vec<DVec2>, mask: &[DVec2]) -> Vec<Vec<DVec2>> {
    let mut remaining = p;
    let mut outside = Vec::new();
    for edge in 0..3 {
        let a = mask[edge];
        let b = mask[(edge + 1) % 3];
        let split = half_plane(&remaining, a, b, -1.);
        if polygon_area(&split) > 1e-20 {
            outside.push(split);
        }
        remaining = half_plane(&remaining, a, b, 1.);
        if remaining.len() < 3 {
            break;
        }
    }
    outside
}

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}

fn area_mm2(t: &PaintTriangle) -> f64 {
    let w = world(t);
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 500_000.
}

#[test]
fn replacement_retains_every_reviewed_source_mask() {
    let old = predecessor();
    let new = support::runtime_map();
    let mut replacement: BTreeMap<SourceKey, Vec<Vec<DVec2>>> = BTreeMap::new();
    for t in &new.triangles {
        replacement
            .entry(source_key(t))
            .or_default()
            .push(source_polygon(t));
    }
    let mut by_part: BTreeMap<&str, [f64; 2]> = BTreeMap::new();
    let mut worst = Vec::new();
    for t in &old.triangles {
        let polygon = source_polygon(t);
        let source_area = polygon_area(&polygon);
        let area = area_mm2(t);
        let mut uncovered = vec![polygon];
        let mut replacement_key = source_key(t);
        replacement_key.3 = corrected_part(t).into();
        if let Some(masks) = replacement.get(&replacement_key) {
            for mask in masks {
                uncovered = uncovered
                    .into_iter()
                    .flat_map(|p| subtract(p, mask))
                    .collect();
                if uncovered.is_empty() {
                    break;
                }
            }
        }
        let missing = if source_area > 1e-20 {
            uncovered.iter().map(|p| polygon_area(p)).sum::<f64>() / source_area * area
        } else {
            0.
        };
        let totals = by_part.entry(&t.part_id).or_default();
        totals[0] += area;
        totals[1] += missing;
        if missing > 0.001 {
            worst.push((missing, t.id.stable_key()));
        }
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    let failures = by_part
        .iter()
        .filter(|(_, totals)| totals[1] > 0.01 + totals[0] * 1e-7)
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "reviewed physical paint was removed: {failures:?}; worst source fragments: {:?}",
        &worst[..worst.len().min(16)]
    );
    let correction = &SOURCE_PART_CORRECTIONS[0];
    assert!(
        !new.triangles.iter().any(|t| {
            t.id.source_mesh == correction.source_mesh
                && t.id.primitive == correction.primitive
                && t.id.source_triangle == correction.source_triangle
                && t.part_id == correction.from_part
        }),
        "the corrected front-left source face must not retain a rear-bumper ghost mask"
    );
    let masks = replacement
        .get(&(
            correction.source_mesh.into(),
            correction.primitive,
            correction.source_triangle,
            correction.to_part.into(),
        ))
        .expect("corrected front-left source face");
    let mut uncovered = vec![vec![DVec2::ZERO, DVec2::X, DVec2::Y]];
    for mask in masks {
        uncovered = uncovered
            .into_iter()
            .flat_map(|p| subtract(p, mask))
            .collect();
    }
    assert!(
        uncovered.iter().map(|p| polygon_area(p)).sum::<f64>() <= 1e-12,
        "the front-left correction must retain the complete original source-face domain"
    );
}

// Independent orthogonal polar frame via sqrt(A^T A), unlike the production
// closed form. This checks image axes as well as migrated source position.
fn polar_axes(t: &PaintTriangle) -> (DVec3, DVec3) {
    let w = world(t);
    let ex = (w[1] - w[0]).normalize();
    let ey = ((w[2] - w[0]) - ex * (w[2] - w[0]).dot(ex)).normalize();
    let physical = DMat2::from_cols(
        DVec2::new((w[1] - w[0]).length(), 0.),
        DVec2::new((w[2] - w[0]).dot(ex), (w[2] - w[0]).dot(ey)),
    ) * 1000.;
    let uv = t.paint_uv.map(|p| DVec2::from_array(p.map(f64::from)));
    let packing = DMat2::from_cols(uv[1] - uv[0], uv[2] - uv[0]) * physical.inverse();
    let gram = packing.transpose() * packing;
    let det_root = gram.determinant().sqrt();
    let positive_root = (gram + DMat2::IDENTITY * det_root)
        / (gram.x_axis.x + gram.y_axis.y + 2. * det_root).sqrt();
    let q = packing * positive_root.inverse();
    (
        ex * q.x_axis.x + ey * q.y_axis.x,
        ex * q.x_axis.y + ey * q.y_axis.y,
    )
}

#[test]
fn reviewed_exterior_anchors_keep_world_position_and_image_axes_after_repacking() {
    let old = predecessor();
    let new = support::runtime_map();
    let migration =
        MappingTransition::with_part_corrections(&old, new, &SOURCE_PART_CORRECTIONS).unwrap();
    let topology = Topology::new(new.triangles.clone(), &new.connections);
    let targets = new
        .triangles
        .iter()
        .map(|t| (&t.id, t))
        .collect::<BTreeMap<_, _>>();
    let mut parts: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, t) in old
        .triangles
        .iter()
        .enumerate()
        .filter(|(_, t)| area_mm2(t) > 1.)
    {
        parts.entry(&t.part_id).or_default().push(i);
    }
    let mut witnesses = BTreeSet::new();
    for indices in parts.values() {
        witnesses.insert(
            *indices
                .iter()
                .max_by(|&&a, &&b| {
                    area_mm2(&old.triangles[a]).total_cmp(&area_mm2(&old.triangles[b]))
                })
                .unwrap(),
        );
        for axis in 0..3 {
            let center = |i: usize| world(&old.triangles[i]).into_iter().sum::<DVec3>()[axis] / 3.;
            witnesses.insert(
                *indices
                    .iter()
                    .min_by(|&&a, &&b| center(a).total_cmp(&center(b)))
                    .unwrap(),
            );
            witnesses.insert(
                *indices
                    .iter()
                    .max_by(|&&a, &&b| center(a).total_cmp(&center(b)))
                    .unwrap(),
            );
        }
    }
    let mut failures = Vec::new();
    let mut corrected_anchors = 0;
    assert_eq!(
        witnesses.len(),
        119,
        "all reviewed physical-anchor witnesses must remain exercised"
    );
    for i in witnesses.iter().copied() {
        let t = &old.triangles[i];
        let prior = world(t).into_iter().sum::<DVec3>() / 3.;
        let (u, v) = polar_axes(t);
        for angle in [0_f32, 0.47, -1.13] {
            let anchor =
                match migration.reproject(&t.id.stable_key(), &t.part_id, Vec3::ONE / 3., angle) {
                    Ok(p) => p,
                    Err(e) => {
                        failures.push(format!("{}: {e}", t.id.stable_key()));
                        continue;
                    }
                };
            let target = targets[&anchor.triangle];
            assert_eq!(anchor.part_id, corrected_part(t));
            assert_eq!(target.part_id, corrected_part(t));
            if corrected_part(t) != t.part_id {
                corrected_anchors += 1;
                assert_eq!(anchor.part_id, "su7.front-left-fender");
                assert_eq!(anchor.triangle.source_mesh, "body.glb/node/175/primitive/0");
                assert_eq!(anchor.triangle.primitive, 0);
                assert_eq!(anchor.triangle.source_triangle, 32590);
            }
            let b = anchor.barycentric.as_dvec3();
            let w = world(target);
            let after = w[0] * b.x + w[1] * b.y + w[2] * b.z;
            assert!(
                after.distance(prior) < 0.00002,
                "saved anchor moved: {}",
                t.id.stable_key()
            );
            // Check the actual public runtime frame used for newly rendered
            // artwork. A raw micro-fragment polar frame can be ill-conditioned
            // even when the stable source/chart runtime frame is correct.
            let (origin, nu, nv) = topology
                .tangent_frame(&anchor.part_id, anchor.paint_uv)
                .unwrap_or_else(|| {
                    panic!(
                        "migrated anchor has no runtime frame: {}",
                        t.id.stable_key()
                    )
                });
            assert!(
                origin.as_dvec3().distance(prior) < 0.00002,
                "migrated public runtime anchor moved: {}",
                t.id.stable_key()
            );
            let (nu, nv) = (nu.as_dvec3().normalize(), nv.as_dvec3().normalize());
            let (s, c) = f64::from(angle).sin_cos();
            let (ns, nc) = f64::from(anchor.rotation_rad).sin_cos();
            let prior_x = u * c + v * s;
            let prior_y = v * c - u * s;
            let next_x = (nu * nc + nv * ns) * if anchor.reflected { -1. } else { 1. };
            let next_y = nv * nc - nu * ns;
            assert!(
                prior_x.dot(next_x) > 0.9999 && prior_y.dot(next_y) > 0.9999,
                "saved image axes changed: {}",
                t.id.stable_key()
            );
        }
    }
    eprintln!(
        "checked {} reviewed physical anchors at 3 rotations",
        witnesses.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        corrected_anchors, 6,
        "both historical misclassified anchors must migrate at all three rotations"
    );
}
