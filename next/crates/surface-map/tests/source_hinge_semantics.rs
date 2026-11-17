//! Independent source-edge hinge witnesses with real bends and excluded masks.
use glam::{DVec3, Vec2};
use itasha_surface_map::{
    EdgeConnection, EdgeRef, MAP_VERSION, PaintSurfaceMap, PaintTriangle, SourceHinge, TriangleId,
    topology::Topology,
};

const ANGLE: f64 = std::f64::consts::FRAC_PI_6;

fn fragment(face: u32, x: f32, angle: f64) -> PaintTriangle {
    let q = if face == 0 {
        [[x - 0.015, 0.005], [x + 0.015, 0.005], [x - 0.015, 0.035]]
    } else {
        [
            [x - 0.015, -0.005],
            [x - 0.015, -0.035],
            [x + 0.015, -0.005],
        ]
    };
    PaintTriangle {
        id: TriangleId {
            source_mesh: "original.glb/node/3/primitive/0".into(),
            primitive: 0,
            source_triangle: face,
            fragment: 1,
        },
        part_id: "panel".into(),
        region_id: "paint".into(),
        chart_id: format!("chart-{face}"),
        world: q.map(|[x, y]| {
            if face == 0 {
                [x, y, 0.]
            } else {
                [
                    x,
                    (f64::from(y) * angle.cos()) as f32,
                    (-f64::from(y) * angle.sin()) as f32,
                ]
            }
        }),
        // Original faces are A,B,C and B,A,D. This witness is independent of
        // the retained edge, which is deliberately five millimetres away.
        source_barycentric: q.map(|[x, y]| {
            if face == 0 {
                [1. - x - 2. * y, x, 2. * y]
            } else {
                [x, 1. - x + 2. * y, -2. * y]
            }
        }),
        paint_uv: q.map(|[x, y]| {
            if face == 0 {
                [x * 1000., y * 1000.]
            } else {
                [3000. - y * 2500. + x * 100., 4000. + x * 700.]
            }
        }),
    }
}

fn fixture(x: f32, intervals: Vec<[f32; 2]>) -> PaintSurfaceMap {
    let triangles = vec![fragment(0, x, ANGLE), fragment(1, x, ANGLE)];
    let connection = EdgeConnection {
        id: "original-shared-edge".into(),
        from: EdgeRef {
            triangle: triangles[0].id.clone(),
            edge: 0,
        },
        to: EdgeRef {
            triangle: triangles[1].id.clone(),
            edge: 0,
        },
        reversed: true,
        physical_gap_mm: 0.,
        propagation: true,
        source_hinge: Some(SourceHinge {
            from_source_edge: 0,
            to_source_edge: 0,
            world: [[0., 0., 0.], [1., 0., 0.]],
            support_intervals: intervals,
        }),
    };
    let mut map = PaintSurfaceMap {
        mapping_version: MAP_VERSION,
        model_fingerprint: "source-hinge-fixture".into(),
        paint_fingerprint: String::new(),
        triangles,
        connections: vec![connection],
    };
    seal(&mut map);
    map
}

fn seal(map: &mut PaintSurfaceMap) {
    map.paint_fingerprint = map.fingerprint();
}

fn center(t: &PaintTriangle) -> Vec2 {
    t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.
}

#[test]
fn a_supported_original_edge_transports_mm_axes_around_a_bend_without_painting_the_gap() {
    let map = fixture(0.2, vec![[0.15, 0.25], [0.7, 0.8]]);
    map.validate().unwrap();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    for source in 0..2 {
        let at = center(&map.triangles[source]);
        let source_world = map.triangles[source]
            .world
            .map(|p| DVec3::from_array(p.map(f64::from)))
            .into_iter()
            .sum::<DVec3>()
            / 3.;
        let (_, u, v) = topology.tangent_frame("panel", at).unwrap();
        let source_axes = [u.as_dvec3().normalize(), v.as_dvec3().normalize()];
        let unbend = |p: DVec3, face: usize| {
            if face == 0 {
                p
            } else {
                DVec3::new(
                    p.x,
                    p.y * ANGLE.cos() - p.z * ANGLE.sin(),
                    p.y * ANGLE.sin() + p.z * ANGLE.cos(),
                )
            }
        };
        let flat_center = unbend(source_world, source);
        let flat_axes = source_axes.map(|a| unbend(a, source));
        let coverage = topology.coverage_at("panel", at.to_array(), [300., 300.], 0., false);
        assert!(
            coverage
                .iter()
                .any(|f| f.triangle.source_triangle == (1 - source) as u32),
            "supported hinge must cross in both directions"
        );
        let mut total_area = 0.;
        for f in coverage {
            let face = f.triangle.source_triangle as usize;
            let points: Vec<_> = f
                .world
                .iter()
                .map(|p| DVec3::from_array(p.map(f64::from)))
                .collect();
            total_area += (points[1] - points[0])
                .cross(points[2] - points[0])
                .length()
                * 0.5
                * 1e6;
            for (p, image) in points.iter().zip(&f.image_uv) {
                let flat = unbend(*p, face);
                assert!(
                    flat.y.abs() >= 0.004999,
                    "hinge must never synthesize paint inside the 10 mm excluded mask gap"
                );
                let delta = (flat - flat_center) * 1000.;
                let expected = Vec2::new(
                    (0.5 + delta.dot(flat_axes[0]) / 300.) as f32,
                    (0.5 + delta.dot(flat_axes[1]) / 300.) as f32,
                );
                assert!(
                    Vec2::from_array(*image).distance(expected) < 2e-5,
                    "independently unbent image coordinates must preserve axes and physical millimetres: face {source}->{face}, expected {expected:?}, got {image:?}"
                );
            }
        }
        assert!(
            (total_area - 900.).abs() < 0.02,
            "only the two existing 450 mm2 masks may be emitted"
        );
    }
}

#[test]
fn artwork_between_multiple_support_intervals_cannot_cross_the_original_edge() {
    let map = fixture(0.5, vec![[0.1, 0.2], [0.8, 0.9]]);
    map.validate().unwrap();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    for source in 0..2 {
        let at = center(&map.triangles[source]);
        let coverage = topology.coverage_at("panel", at.to_array(), [100., 100.], 0., false);
        assert!(!coverage.is_empty());
        assert!(
            coverage
                .iter()
                .all(|f| f.triangle.source_triangle == source as u32),
            "an image footprint in the large excluded support gap cannot propagate merely because its source faces share an edge"
        );
    }
}

#[test]
fn even_a_small_footprint_overlap_with_one_support_interval_allows_crossing() {
    let map = fixture(0.245, vec![[0.15, 0.2], [0.8, 0.9]]);
    map.validate().unwrap();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    // Source center is x=240mm, so the footprint meets the first support only
    // over x=190..200mm; the second support is irrelevant.
    let at = center(&map.triangles[0]);
    let coverage = topology.coverage_at("panel", at.to_array(), [100., 100.], 0., false);
    assert!(coverage.iter().any(|f| f.triangle.source_triangle == 1));
}

#[test]
fn only_reconstructed_original_edge_geometry_within_five_micrometres_is_accepted() {
    let valid = fixture(0.2, vec![[0.15, 0.25]]);
    valid.validate().unwrap();
    let mut within = valid.clone();
    for p in &mut within.triangles[1].world {
        p[2] += 0.000004;
    }
    seal(&mut within);
    within.validate().expect(
        "a four micrometre endpoint residual remains inside the explicit reconstruction tolerance",
    );
    let invalid = |mut map: PaintSurfaceMap, label: &str| {
        seal(&mut map);
        assert!(
            map.validate().is_err(),
            "invalid source hinge must reject {label}"
        );
    };
    let mut map = valid.clone();
    map.triangles[1] = fragment(1, 0.2, std::f64::consts::PI);
    invalid(map, "coincident inward-facing duplicate sheet");
    let mut map = valid.clone();
    map.triangles[1].part_id = "another-panel".into();
    invalid(map, "different paint part");
    let mut map = valid.clone();
    map.connections[0].source_hinge.as_mut().unwrap().world[0][2] = 0.001;
    invalid(
        map,
        "invented world hinge away from original source corners",
    );
    let mut map = valid.clone();
    for p in &mut map.triangles[1].world {
        p[2] += 0.000006;
    }
    invalid(map, "six micrometre original-edge endpoint mismatch");
    let mut map = valid.clone();
    map.connections[0]
        .source_hinge
        .as_mut()
        .unwrap()
        .from_source_edge = 1;
    invalid(map, "incorrect original source edge ordinal");
    for intervals in [
        vec![],
        vec![[0.4, 0.4]],
        vec![[0.8, 0.2]],
        vec![[-0.1, 0.2]],
        vec![[0.8, 1.1]],
    ] {
        let mut map = valid.clone();
        map.connections[0]
            .source_hinge
            .as_mut()
            .unwrap()
            .support_intervals = intervals;
        invalid(map, "empty or invalid reviewed support intervals");
    }
}
