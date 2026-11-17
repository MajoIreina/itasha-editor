//! Exercise the reviewed vehicle graph through actual local coverage queries,
//! rather than accepting a connected part graph as proof of working artwork.
use glam::{Vec2, Vec3};
use itasha_surface_map::{
    CoverageFragment, EdgeConnection, PaintSurfaceMap, PaintTriangle, topology::Topology,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
mod support;

fn centroid(t: &PaintTriangle) -> Vec3 {
    t.world.map(Vec3::from_array).into_iter().sum::<Vec3>() / 3.
}
fn visible_area(fragments: &[CoverageFragment], part: &str) -> f64 {
    fragments
        .iter()
        .filter(|f| f.part_id == part)
        .map(|f| {
            let p = f
                .world
                .iter()
                .copied()
                .map(Vec3::from_array)
                .collect::<Vec<_>>();
            f64::from((p[1] - p[0]).cross(p[2] - p[0]).length()) * 0.5
        })
        .sum()
}

// A common exterior viewing direction gives an independent image-Jacobian
// oracle even when neighboring charts have unrelated rotations or scales.
fn handedness(fragments: &[CoverageFragment], part: &str, origin: Vec3, outward: Vec3) -> [f64; 2] {
    let mut area = [0.; 2];
    for f in fragments.iter().filter(|f| f.part_id == part) {
        let w = f
            .world
            .iter()
            .copied()
            .map(Vec3::from_array)
            .collect::<Vec<_>>();
        let n = (w[1] - w[0]).cross(w[2] - w[0]);
        if n.length() < 1e-10
            || n.dot(outward).abs() < n.length() * 0.35
            || ((w[0] + w[1] + w[2]) / 3.).distance(origin) > 0.4
        {
            continue;
        }
        let uv = f
            .image_uv
            .iter()
            .copied()
            .map(Vec2::from_array)
            .collect::<Vec<_>>();
        let d = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]);
        if d.abs() < 1e-12 {
            continue;
        }
        area[usize::from(d * n.dot(outward) > 0.)] += f64::from(n.dot(outward).abs()) * 0.5;
    }
    area
}

#[test]
fn every_reviewed_body_seam_transports_local_artwork_in_both_directions() {
    let map = support::runtime_map();
    let old_path = support::paint_root().join("su7-v21-classified-only/paint-map.json");
    let old: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(old_path).unwrap()).unwrap();
    old.validate().unwrap();
    assert_eq!(
        old.paint_fingerprint,
        "b16c24a5ff3ba086c545841dd92fd5d0a99ea50ab0cd270b94caca9d8b767db1"
    );
    assert_eq!(map.mapping_version, old.mapping_version);
    assert_eq!(map.model_fingerprint, old.model_fingerprint);
    let lower_fender_faces = map
        .triangles
        .iter()
        .filter(|t| t.part_id == "su7.front-left-lower-fender")
        .map(|t| t.id.source_triangle)
        .collect::<BTreeSet<_>>();
    assert!(
        lower_fender_faces.len() > 842,
        "left lower fender must recover reviewed faces beyond the partial v22 set"
    );
    assert_ne!(map.connections, old.connections);
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let triangles: HashMap<_, _> = map.triangles.iter().map(|t| (&t.id, t)).collect();
    assert_eq!(
        map.connections
            .iter()
            .filter(|c| triangles[&c.from.triangle].part_id != triangles[&c.to.triangle].part_id)
            .count(),
        730,
        "retain all reviewed cross-part seams independently of same-part source hinges"
    );
    let mut pairs: BTreeMap<(&str, &str), Vec<&EdgeConnection>> = BTreeMap::new();
    for c in map.connections.iter().filter(|c| c.propagation) {
        let a = triangles[&c.from.triangle].part_id.as_str();
        let b = triangles[&c.to.triangle].part_id.as_str();
        if a != b {
            pairs.entry((a.min(b), a.max(b))).or_default().push(c);
        }
    }
    let mut required = BTreeSet::new();
    for side in ["left", "right"] {
        for (a, b) in [
            (
                format!("su7.front-{side}-door"),
                format!("su7.rear-{side}-door"),
            ),
            (
                format!("su7.front-{side}-door"),
                format!("su7.front-{side}-fender"),
            ),
            (
                format!("su7.front-{side}-door"),
                format!("su7.front-{side}-lower-fender"),
            ),
            (
                format!("su7.front-{side}-door"),
                format!("su7.rear-{side}-fender"),
            ),
            (
                format!("su7.front-{side}-fender"),
                format!("su7.rear-{side}-fender"),
            ),
            (
                format!("su7.rear-{side}-door"),
                format!("su7.rear-{side}-fender"),
            ),
            (
                format!("su7.front-{side}-fender"),
                format!("su7.front-{side}-lower-fender"),
            ),
            (
                "su7.front-bumper".into(),
                format!("su7.front-{side}-fender"),
            ),
            ("su7.hood".into(), format!("su7.front-{side}-fender")),
            ("su7.rear-bumper".into(), format!("su7.rear-{side}-fender")),
        ] {
            required.insert((a.clone().min(b.clone()), a.max(b)));
        }
    }
    for (a, b) in [
        ("su7.front-bumper", "su7.hood"),
        ("su7.charge-cover", "su7.rear-left-fender"),
        ("su7.front-bumper", "su7.front-c-ring"),
        ("su7.rear-bumper", "su7.rear-c-ring"),
    ] {
        required.insert((a.min(b).to_owned(), a.max(b).to_owned()));
    }
    assert_eq!(pairs.len(), 24);
    for (a, b) in required {
        assert!(
            pairs.contains_key(&(a.as_str(), b.as_str())),
            "missing reviewed physical adjacency {a} / {b}"
        );
    }
    assert!(
        !pairs
            .keys()
            .any(|(a, b)| *a == "su7.rear-spoiler" || *b == "su7.rear-spoiler"),
        "floating spoiler is separate; artwork must not jump through open air"
    );
    let mut queries = 0;
    let mut failures = Vec::new();
    let mut orientation_failures = Vec::new();
    let mut orientation_queries = 0;
    let mut max_fold_fraction = 0_f64;
    for ((a, b), mut connections) in pairs {
        let midpoint = |c: &EdgeConnection| {
            (centroid(triangles[&c.from.triangle]) + centroid(triangles[&c.to.triangle])) * 0.5
        };
        let points = connections.iter().map(|c| midpoint(c)).collect::<Vec<_>>();
        let min = points
            .iter()
            .copied()
            .fold(Vec3::splat(f32::INFINITY), Vec3::min);
        let max = points
            .iter()
            .copied()
            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
        let span = max - min;
        let axis = (0..3).max_by(|&a, &b| span[a].total_cmp(&span[b])).unwrap();
        connections.sort_by(|a, b| {
            midpoint(a)[axis]
                .total_cmp(&midpoint(b)[axis])
                .then(a.id.cmp(&b.id))
        });
        let samples = [
            connections.len() / 4,
            connections.len() / 2,
            connections.len() * 3 / 4,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        for index in samples {
            let c = connections[index];
            for (source, target) in [
                (triangles[&c.from.triangle], triangles[&c.to.triangle]),
                (triangles[&c.to.triangle], triangles[&c.from.triangle]),
            ] {
                let center = source
                    .paint_uv
                    .map(Vec2::from_array)
                    .into_iter()
                    .sum::<Vec2>()
                    / 3.;
                for size in [300., 700.] {
                    queries += 1;
                    let coverage = topology.coverage_at(
                        &source.part_id,
                        center.to_array(),
                        [size, size],
                        0.17,
                        true,
                    );
                    // Artwork dimensions are physical millimetres. Distortion
                    // in a packed chart cannot exempt a real, nearby seam.
                    if visible_area(&coverage, &target.part_id) <= 1e-7 {
                        failures.push(format!("{a} / {b}: {} -> {} at {size}mm does not cross sampled seam {} (center={center:?}; source={})", source.part_id, target.part_id, c.id, source.id.stable_key()));
                    }
                    if size == 700. {
                        let origin = (centroid(source) + centroid(target)) * 0.5;
                        let outward = (origin - Vec3::new(0., 0.7, 0.)).normalize();
                        let sa = handedness(&coverage, &source.part_id, origin, outward);
                        let ta = handedness(&coverage, &target.part_id, origin, outward);
                        if sa.iter().sum::<f64>() > 1e-6 && ta.iter().sum::<f64>() > 1e-6 {
                            orientation_queries += 1;
                            for area in [sa, ta] {
                                max_fold_fraction = max_fold_fraction
                                    .max(area[0].min(area[1]) / (area[0] + area[1]));
                            }
                            if (sa[1] > sa[0]) != (ta[1] > ta[0]) {
                                orientation_failures.push(format!(
                                    "{} -> {} {}: source={sa:?} target={ta:?}",
                                    source.part_id, target.part_id, c.id
                                ));
                            }
                        }
                    }
                    assert!(
                        coverage
                            .iter()
                            .flat_map(|f| &f.image_uv)
                            .flatten()
                            .all(|v| v.is_finite() && (-1e-5..=1.00001).contains(v))
                    );
                    if size == 300. {
                        for (side, opposite) in [("-left-", "-right-"), ("-right-", "-left-")] {
                            if source.part_id.contains(side) {
                                let jumped: Vec<_> = coverage
                                    .iter()
                                    .filter(|f| f.part_id.contains(opposite))
                                    .map(|f| f.part_id.clone())
                                    .collect();
                                assert!(
                                    jumped.is_empty(),
                                    "small artwork must not jump across the vehicle: {a} / {b} seam {} source {} reached {:?}",
                                    c.id,
                                    source.part_id,
                                    jumped
                                );
                            }
                        }
                    }
                    let source_only = topology.coverage_at(
                        &source.part_id,
                        center.to_array(),
                        [size, size],
                        0.17,
                        false,
                    );
                    assert_eq!(
                        coverage
                            .iter()
                            .filter(|f| f.part_id == source.part_id)
                            .cloned()
                            .collect::<Vec<_>>(),
                        source_only,
                        "crossing other panels must not alter the source sheet"
                    );
                }
            }
        }
    }
    assert_eq!(topology.triangles, map.triangles);
    assert_eq!(
        // Reviewed v25 emits 142 directional samples: pairs with fewer than
        // three distinct quartile connections contribute fewer slots. Every
        // selected 700mm sample exposes both skins to the common exterior view.
        orientation_queries,
        142,
        "the reviewed exterior-facing Jacobian witness set changed and needs inspection"
    );
    assert_eq!(orientation_queries * 2, queries);
    assert!(
        max_fold_fraction < 0.25,
        "the local exterior sheet must retain a clear dominant handedness"
    );
    eprintln!("verified {queries} local whole-car coverage queries");
    eprintln!(
        "verified {orientation_queries} world Jacobian orientations; max projected fold fraction {max_fold_fraction}"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        orientation_failures.is_empty(),
        "{}",
        orientation_failures.join("\n")
    );
}

#[test]
fn actual_side_panels_transport_one_artwork_across_three_parts() {
    let map = support::runtime_map();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    for side in ["left", "right"] {
        let part = format!("su7.front-{side}-door");
        let seed = map
            .triangles
            .iter()
            .filter(|t| t.part_id == part)
            .min_by(|a, b| {
                let distance = |t: &PaintTriangle| {
                    let p = centroid(t);
                    (p.x - 0.05).powi(2) + (p.y - 0.72).powi(2)
                };
                distance(a).total_cmp(&distance(b))
            })
            .unwrap();
        let center = seed
            .paint_uv
            .map(Vec2::from_array)
            .into_iter()
            .sum::<Vec2>()
            / 3.;
        let coverage = topology.coverage_at(&part, center.to_array(), [2800., 1800.], 0., true);
        for target in [
            part.clone(),
            format!("su7.rear-{side}-door"),
            format!("su7.rear-{side}-fender"),
        ] {
            assert!(
                visible_area(&coverage, &target) > 0.001,
                "{part} -> {target} must carry the same artwork through a real multi-part chain"
            );
        }
        assert!(!coverage.iter().any(|f| f.part_id == "su7.rear-spoiler"));
    }
}
