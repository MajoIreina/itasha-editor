use glam::{Vec2, Vec3};
use itasha_surface_map::{PaintSurfaceMap, barycentric_2d, topology::Topology};
use std::{collections::HashMap, path::Path, sync::OnceLock};

fn classified_map() -> &'static PaintSurfaceMap {
    static MAP: OnceLock<PaintSurfaceMap> = OnceLock::new();
    MAP.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/paint/su7-v21-classified-only/paint-map.json");
        serde_json::from_slice(&std::fs::read(path).expect("committed classified SU7 paint map"))
            .expect("valid classified SU7 paint map")
    })
}

#[test]
fn classified_door_artwork_reaches_the_neighbor_without_changing_the_map() {
    let map = classified_map();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let triangles: HashMap<_, _> = map.triangles.iter().map(|t| (&t.id, t)).collect();
    // These centers lie at world y = 0.70 m near the real front/rear door seam.
    // Each source chart retains the coordinates supplied by the classified AO map.
    for (source, neighbor, center) in [
        (
            "su7.rear-left-door",
            "su7.front-left-door",
            [422.30735, 160.68070],
        ),
        (
            "su7.rear-right-door",
            "su7.front-right-door",
            [411.78792, 161.49771],
        ),
        (
            "su7.front-left-door",
            "su7.rear-left-door",
            [414.37325, 1071.0663],
        ),
        (
            "su7.front-right-door",
            "su7.rear-right-door",
            [351.94588, 1071.3204],
        ),
    ] {
        for size in [300., 400.] {
            let coverage = topology.coverage_at(source, center, [size, size], 0., true);
            let count = |part: &str| coverage.iter().filter(|f| f.part_id == part).count();
            assert!(
                count(source) > 100,
                "{source}: source door must retain the artwork"
            );
            assert!(
                count(neighbor) > 0,
                "{neighbor}: the other half must cross the real seam"
            );
            assert!(
                coverage
                    .iter()
                    .all(|f| f.part_id == source || f.part_id == neighbor)
            );
            for fragment in &coverage {
                let original = triangles[&fragment.triangle];
                assert_eq!(fragment.world.len(), fragment.image_uv.len());
                assert_eq!(fragment.world.len(), fragment.paint_uv.len());
                for ((world, paint), image) in fragment
                    .world
                    .iter()
                    .zip(&fragment.paint_uv)
                    .zip(&fragment.image_uv)
                {
                    assert!(
                        image
                            .iter()
                            .all(|v| v.is_finite() && *v >= -0.00001 && *v <= 1.00001)
                    );
                    assert!(paint.iter().all(|v| v.is_finite()));
                    let b = barycentric_2d(
                        Vec2::from_array(*paint),
                        original.paint_uv.map(Vec2::from_array),
                    )
                    .expect("classified paint coordinates remain invertible");
                    let expected = Vec3::from_array(original.world[0]) * b.x
                        + Vec3::from_array(original.world[1]) * b.y
                        + Vec3::from_array(original.world[2]) * b.z;
                    assert!(
                        expected.distance(Vec3::from_array(*world)) < 0.0002,
                        "coverage must recover original classified paint coordinates: {}",
                        original.id.stable_key()
                    );
                }
            }

            let single = topology.coverage_at(source, center, [size, size], 0., false);
            assert!(!single.is_empty());
            assert!(single.iter().all(|f| f.part_id == source));
            let source_cross: Vec<_> = coverage
                .iter()
                .filter(|f| f.part_id == source)
                .cloned()
                .collect();
            assert_eq!(
                source_cross, single,
                "enabling cross-surface coverage must not change source fragments"
            );
        }
    }
    assert_eq!(
        topology.triangles, map.triangles,
        "cross-surface coverage must preserve the classified mapping"
    );
}

#[test]
fn large_asymmetric_artwork_keeps_the_same_world_handedness_on_both_doors() {
    let map = classified_map();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    for (source, neighbor, source_x) in [
        ("su7.front-left-door", "su7.rear-left-door", 0.05),
        ("su7.rear-left-door", "su7.front-left-door", 0.35),
        ("su7.front-right-door", "su7.rear-right-door", 0.05),
        ("su7.rear-right-door", "su7.front-right-door", 0.35),
    ] {
        for y in [0.42, 0.58, 0.59, 0.72, 0.88] {
            let seed = map
                .triangles
                .iter()
                .filter(|t| t.part_id == source)
                .min_by(|a, b| {
                    let distance = |t: &itasha_surface_map::PaintTriangle| {
                        let p = t.world.map(Vec3::from_array).into_iter().sum::<Vec3>() / 3.;
                        (p.x - source_x).powi(2) + (p.y - y).powi(2)
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
            let fragments =
                topology.coverage_at(source, center.to_array(), [1800., 1800.], 0.31, true);
            // The outer central door skin is locally a graph over world X/Y.
            // Its image Jacobian must have the same handedness on both doors:
            // U/V encode independent color gradients in an asymmetric test image.
            let orientation = |part: &str| {
                let mut area = [0_f64; 2];
                for f in fragments.iter().filter(|f| f.part_id == part) {
                    let w = f
                        .world
                        .iter()
                        .copied()
                        .map(Vec3::from_array)
                        .collect::<Vec<_>>();
                    let normal = (w[1] - w[0]).cross(w[2] - w[0]);
                    let center = (w[0] + w[1] + w[2]) / 3.;
                    if normal.length() < 1e-10
                        || normal.z.abs() / normal.length() < 0.8
                        || !(0.35..0.95).contains(&center.y)
                    {
                        continue;
                    }
                    let uv = f
                        .image_uv
                        .iter()
                        .copied()
                        .map(Vec2::from_array)
                        .collect::<Vec<_>>();
                    let image_area = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]);
                    let sign = usize::from(image_area * normal.z > 0.);
                    area[sign] += f64::from(normal.z.abs()) * 0.5;
                }
                assert!(
                    area[0] + area[1] > 0.02,
                    "{part} at {y}: enough visible skin must receive the artwork"
                );
                let dominant = usize::from(area[1] > area[0]);
                assert!(
                    area[1 - dominant] < (area[0] + area[1]) * 0.01,
                    "{part} at {y}: local handedness folds: {area:?}"
                );
                dominant
            };
            assert_eq!(
                orientation(source),
                orientation(neighbor),
                "{source} -> {neighbor} at y={y}: an asymmetric image must not be mirrored at the seam"
            );
        }
    }
}
