//! A clipped paint mask remains part of its original affine source plane.
//! Artwork may extend to another retained mask fragment without painting the gap.
use glam::{DVec3, Vec2};
use itasha_surface_map::{PaintTriangle, TriangleId, topology::Topology};
use std::collections::BTreeSet;

fn triangle(
    fragment: u64,
    source: u32,
    part: &str,
    points: [[f32; 2]; 3],
    chart: &str,
    reflected: bool,
) -> PaintTriangle {
    PaintTriangle {
        id: TriangleId {
            source_mesh: "original.glb/node/7/primitive/0".into(),
            primitive: 0,
            source_triangle: source,
            fragment,
        },
        part_id: part.into(),
        region_id: "paint".into(),
        chart_id: chart.into(),
        world: points.map(|p| [p[0], p[1], 0.]),
        source_barycentric: points.map(|p| [1. - p[0] - p[1], p[0], p[1]]),
        paint_uv: points.map(|p| {
            if reflected {
                [1000. + p[1] * 1000., 1000. + p[0] * 1000.]
            } else {
                [p[0] * 1000., p[1] * 1000.]
            }
        }),
    }
}
fn masks() -> Vec<PaintTriangle> {
    vec![
        triangle(
            1,
            0,
            "panel",
            [[0., 0.], [0.03, 0.], [0., 0.03]],
            "left",
            false,
        ),
        triangle(
            2,
            0,
            "panel",
            // Opposite stored winding plus a reflected packed chart must not
            // reflect the source-plane image affine.
            [[0.08, 0.], [0.08, 0.03], [0.11, 0.]],
            "right-reflected",
            true,
        ),
        triangle(
            3,
            1,
            "panel",
            [[0.04, 0.04], [0.06, 0.04], [0.04, 0.06]],
            "unrelated-source",
            false,
        ),
        triangle(
            4,
            0,
            "other-part",
            [[0.04, 0.], [0.06, 0.], [0.04, 0.02]],
            "other-part",
            false,
        ),
    ]
}

#[test]
fn disjoint_masks_on_one_original_face_share_artwork_affine_without_filling_gap() {
    let topology = Topology::new(masks(), &[]);
    let coverage = topology.coverage_at("panel", [10., 10.], [300., 300.], 0., false);
    let ids = coverage
        .iter()
        .map(|f| f.triangle.fragment)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids,
        BTreeSet::from([1, 2]),
        "same source/part must extend through retained masks, while other source/part stays isolated"
    );
    let mut total_area = 0.;
    for f in coverage {
        let w = f
            .world
            .iter()
            .map(|p| DVec3::from_array(p.map(f64::from)))
            .collect::<Vec<_>>();
        total_area += (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6;
        for (point, image) in f.world.iter().zip(&f.image_uv) {
            let x = f64::from(point[0]) * 1000.;
            let y = f64::from(point[1]) * 1000.;
            assert!(
                x <= 30.001 || x >= 79.999,
                "retained geometry cannot fill the 50 mm mask gap"
            );
            let expected = Vec2::new(
                ((x - 10. + 150.) / 300.) as f32,
                ((y - 10. + 150.) / 300.) as f32,
            );
            assert!(
                Vec2::from_array(*image).distance(expected) < 2e-6,
                "image affine must be source-world continuous despite reflected packed target chart"
            );
        }
    }
    assert!(
        (total_area - 900.).abs() < 0.01,
        "only the two original 450 mm2 masks may be emitted"
    );
}

#[test]
fn a_matching_source_label_cannot_import_an_inconsistent_world_sheet() {
    let mut triangles = masks();
    let mut corrupt = triangle(
        5,
        0,
        "panel",
        [[0.04, 0.04], [0.042, 0.04], [0.04, 0.042]],
        "invalid-source-world",
        false,
    );
    corrupt.world = corrupt.world.map(|p| [p[0], p[1], 0.01]);
    triangles.push(corrupt);
    let topology = Topology::new(triangles, &[]);
    let coverage = topology.coverage_at("panel", [10., 10.], [300., 300.], 0., false);
    assert!(
        !coverage.iter().any(|f| f.triangle.fragment == 5),
        "same source metadata with a 10 mm world-plane mismatch must not join the trusted source affine group"
    );
}

#[test]
fn a_far_stable_reference_does_not_prune_nearby_masks_on_the_same_source_plane() {
    let triangles = vec![
        triangle(
            11,
            9,
            "panel",
            [[0., 0.], [0.003, 0.], [0., 0.003]],
            "tiny-seed",
            false,
        ),
        triangle(
            12,
            9,
            "panel",
            [[0.6, 0.], [0.7, 0.], [0.6, 0.1]],
            "far-stable-reference",
            false,
        ),
        triangle(
            13,
            9,
            "panel",
            [[0.02, 0.], [0.02, 0.003], [0.023, 0.]],
            "nearby-mask-reflected",
            true,
        ),
    ];
    let topology = Topology::new(triangles, &[]);
    // The large reference is 600 mm away, while the third mask is only 20 mm
    // from this 80 mm artwork. A reference-only star must remain traversable.
    let coverage = topology.coverage_at("panel", [1., 1.], [80., 80.], 0., false);
    let ids = coverage
        .iter()
        .map(|f| f.triangle.fragment)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids,
        BTreeSet::from([11, 13]),
        "source-plane reference traversal must reach the nearby mask and strictly clip the far reference"
    );
    let mut total_area = 0.;
    for f in coverage {
        let w = f
            .world
            .iter()
            .map(|p| DVec3::from_array(p.map(f64::from)))
            .collect::<Vec<_>>();
        total_area += (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6;
        for (point, image) in f.world.iter().zip(&f.image_uv) {
            let x = f64::from(point[0]) * 1000.;
            let y = f64::from(point[1]) * 1000.;
            assert!(
                x <= 3.001 || (19.999..=23.001).contains(&x),
                "source-plane traversal cannot emit the mask gap or far reference"
            );
            let expected = Vec2::new(((x - 1. + 40.) / 80.) as f32, ((y - 1. + 40.) / 80.) as f32);
            assert!(
                Vec2::from_array(*image).distance(expected) < 2e-6,
                "nearby mask must retain the seed's image affine"
            );
        }
    }
    assert!(
        (total_area - 9.).abs() < 0.001,
        "only the two retained 4.5 mm2 nearby masks may be emitted"
    );
}
