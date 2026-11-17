//! Geometric oracles for propagation: the expected image coordinates come from
//! the known flat or bent sheet, independently of the triangle traversal.
use glam::{Mat2, Vec2, Vec3};
use itasha_surface_map::{
    CoverageFragment, CoverageRect, EdgeConnection, EdgeRef, PaintTriangle, TriangleId,
    barycentric_2d, coverage_triangles, topology::Topology,
};
use std::collections::BTreeMap;

fn panel(first_id: u32, part: &str, world: [[f32; 3]; 4], uv: [[f32; 2]; 4]) -> Vec<PaintTriangle> {
    [[0, 1, 2], [1, 3, 2]]
        .into_iter()
        .enumerate()
        .map(|(offset, vertices)| PaintTriangle {
            id: TriangleId {
                source_mesh: "metric-fixture".into(),
                primitive: 0,
                source_triangle: first_id + offset as u32,
                fragment: 0,
            },
            part_id: part.into(),
            region_id: "exterior".into(),
            chart_id: format!("{part}-chart"),
            world: vertices.map(|i| world[i]),
            paint_uv: vertices.map(|i| uv[i]),
            source_barycentric: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        })
        .collect()
}

fn flat_panel(
    first_id: u32,
    part: &str,
    x: f32,
    y: f32,
    height: f32,
    uv: impl Fn(Vec2) -> Vec2,
) -> Vec<PaintTriangle> {
    let points = [
        Vec2::new(x, y),
        Vec2::new(x + 1., y),
        Vec2::new(x, y + height),
        Vec2::new(x + 1., y + height),
    ];
    panel(
        first_id,
        part,
        points.map(|p| [p.x, p.y, 0.]),
        points.map(|p| uv(p * 1000.).to_array()),
    )
}

fn join(left: &[PaintTriangle], right: &[PaintTriangle], gap: f32) -> EdgeConnection {
    EdgeConnection {
        id: format!("{}-to-{}", left[0].part_id, right[0].part_id),
        from: EdgeRef {
            triangle: left[1].id.clone(),
            edge: 0,
        },
        to: EdgeRef {
            triangle: right[0].id.clone(),
            edge: 2,
        },
        reversed: true,
        physical_gap_mm: gap,
        propagation: true,
    }
}

fn seed_uv(p: Vec2) -> Vec2 {
    // Deliberately anisotropic and skewed. Applying physical lengths directly
    // inside this chart would change the metric at every subsequent triangle.
    Vec2::new(2. * p.x + 0.5 * p.y + 100., -0.4 * p.x + 0.8 * p.y + 200.)
}

fn foreign_uv(p: Vec2) -> Vec2 {
    Vec2::new(8100. - 0.3 * p.y, -4200. + 3.1 * p.x + 0.2 * p.y)
}

fn image_area_by_part(fragments: &[CoverageFragment]) -> BTreeMap<String, f32> {
    let mut areas = BTreeMap::new();
    for f in fragments {
        let p = f
            .image_uv
            .iter()
            .copied()
            .map(Vec2::from_array)
            .collect::<Vec<_>>();
        let area = (p[1] - p[0]).perp_dot(p[2] - p[0]).abs() * 0.5;
        *areas.entry(f.part_id.clone()).or_default() += area;
    }
    areas
}

fn assert_matches_sheet(
    triangles: &[PaintTriangle],
    fragments: &[CoverageFragment],
    rect: CoverageRect,
    domain: impl Fn([f32; 3]) -> Vec2,
) {
    let originals = triangles
        .iter()
        .map(|t| (&t.id, t))
        .collect::<BTreeMap<_, _>>();
    let rotation = Mat2::from_angle(-rect.rotation_rad);
    for fragment in fragments {
        let original = originals[&fragment.triangle];
        for ((world, image), paint) in fragment
            .world
            .iter()
            .zip(&fragment.image_uv)
            .zip(&fragment.paint_uv)
        {
            let expected = rotation * (domain(*world) - rect.center) / rect.size + Vec2::splat(0.5);
            assert!(
                expected.distance(Vec2::from_array(*image)) < 0.0001,
                "{}: image restarts, changes scale or folds; world={world:?}, expected={expected:?}, actual={image:?}",
                fragment.part_id
            );
            let b = barycentric_2d(
                Vec2::from_array(*paint),
                original.paint_uv.map(Vec2::from_array),
            )
            .unwrap();
            let w = original.world.map(Vec3::from_array);
            let recovered = w[0] * b.x + w[1] * b.y + w[2] * b.z;
            assert!(
                recovered.distance(Vec3::from_array(*world)) < 0.0002,
                "propagation must preserve the original display chart coordinates"
            );
        }
    }
    // The oracle knows the entire sheet's flat coordinates, so it needs no
    // graph search. Comparing area also catches omitted or duplicated coverage.
    let expected_triangles = triangles
        .iter()
        .cloned()
        .map(|mut t| {
            t.paint_uv = t.world.map(|p| domain(p).to_array());
            t
        })
        .collect::<Vec<_>>();
    let expected = coverage_triangles(expected_triangles.iter(), rect, None);
    let expected_area = image_area_by_part(&expected);
    let actual_area = image_area_by_part(fragments);
    assert_eq!(
        actual_area.keys().collect::<Vec<_>>(),
        expected_area.keys().collect::<Vec<_>>()
    );
    for (part, area) in expected_area {
        assert!(
            (actual_area[&part] - area).abs() < 0.0001,
            "{part}: missing or duplicated sheet coverage: expected={area}, actual={}",
            actual_area[&part]
        );
    }
}

#[test]
fn anisotropic_seed_extends_consistently_across_an_unrelated_chart() {
    let source = flat_panel(0, "source", 0., 0., 1., seed_uv);
    let neighbor = flat_panel(2, "neighbor", 1., 0., 1., foreign_uv);
    let connection = join(&source, &neighbor, 0.);
    let triangles = [source, neighbor].concat();
    let topology = Topology::new(triangles.clone(), &[connection]);
    let rect = CoverageRect {
        center: seed_uv(Vec2::new(800., 400.)),
        size: Vec2::new(2200., 1200.),
        rotation_rad: 0.17,
    };
    let actual = topology.coverage("source", rect, true);
    assert!(actual.iter().any(|f| f.part_id == "neighbor"));
    assert_matches_sheet(&triangles, &actual, rect, |w| {
        seed_uv(Vec2::new(w[0], w[1]) * 1000.)
    });
    assert_eq!(topology.triangles, triangles);
}

#[test]
fn ninety_degree_bend_uses_surface_distance_and_keeps_the_real_gap() {
    let source = flat_panel(0, "source", 0., 0., 1., |p| p);
    let neighbor = panel(
        2,
        "folded",
        [
            [1.003, 0., 0.],
            [1.003, 0., 1.],
            [1.003, 1., 0.],
            [1.003, 1., 1.],
        ],
        [
            Vec2::ZERO,
            Vec2::new(1000., 0.),
            Vec2::new(0., 1000.),
            Vec2::splat(1000.),
        ]
        .map(|p| foreign_uv(p).to_array()),
    );
    let connection = join(&source, &neighbor, 3.);
    let triangles = [source, neighbor].concat();
    let topology = Topology::new(triangles.clone(), &[connection]);
    let rect = CoverageRect {
        center: Vec2::new(850., 450.),
        size: Vec2::new(1200., 800.),
        rotation_rad: 0.12,
    };
    let actual = topology.coverage("source", rect, true);
    assert!(actual.iter().any(|f| f.part_id == "folded"));
    assert_matches_sheet(&triangles, &actual, rect, |w| {
        Vec2::new((w[0] + w[2]) * 1000., w[1] * 1000.)
    });
}

#[test]
fn unequal_seam_edges_preserve_target_length_and_tangential_offset() {
    let source = flat_panel(0, "source", 0., 0., 1., seed_uv);
    // Endpoint offsets along the seam are 1 mm and 3 mm. The target edge is
    // 1002 mm long; copying the source's 1000 mm edge would distort this sheet.
    let neighbor = flat_panel(2, "neighbor", 1.003, 0.001, 1.002, foreign_uv);
    let connection = join(&source, &neighbor, 3.);
    let triangles = [source, neighbor].concat();
    let topology = Topology::new(triangles.clone(), &[connection]);
    let rect = CoverageRect {
        center: seed_uv(Vec2::new(850., 450.)),
        size: Vec2::new(2300., 1600.),
        rotation_rad: 0.,
    };
    let actual = topology.coverage("source", rect, true);
    assert!(actual.iter().any(|f| f.part_id == "neighbor"));
    assert_matches_sheet(&triangles, &actual, rect, |w| {
        seed_uv(Vec2::new(w[0], w[1]) * 1000.)
    });
}

#[test]
fn a_chain_of_neighbors_preserves_one_image_and_does_not_change_the_anchor() {
    let source = flat_panel(0, "source", 0., 0., 1., seed_uv);
    let middle = flat_panel(2, "middle", 1.003, 0., 1., foreign_uv);
    let far = flat_panel(4, "far", 2.007, 0., 1., |p| {
        Vec2::new(-p.x + 9000., -0.7 * p.y + 5000.)
    });
    let connections = [join(&source, &middle, 3.), join(&middle, &far, 4.)];
    let triangles = [source, middle, far].concat();
    let topology = Topology::new(triangles.clone(), &connections);
    let rect = CoverageRect {
        center: seed_uv(Vec2::new(850., 450.)),
        size: Vec2::new(8000., 2800.),
        rotation_rad: 0.,
    };
    let actual = topology.coverage("source", rect, true);
    assert!(actual.iter().any(|f| f.part_id == "far"));
    assert_matches_sheet(&triangles, &actual, rect, |w| {
        seed_uv(Vec2::new(w[0], w[1]) * 1000.)
    });
    let source_only = topology.coverage("source", rect, false);
    assert!(!source_only.is_empty());
    assert!(source_only.iter().all(|f| f.part_id == "source"));
    assert_eq!(
        actual
            .iter()
            .filter(|f| f.part_id == "source")
            .cloned()
            .collect::<Vec<_>>(),
        source_only
    );
}

#[test]
fn independent_seam_samples_keep_world_orientation_even_when_triangles_point_the_same_way() {
    // A classified boundary can cut two nearby source triangles through the
    // same side of their sampled edge. These are seam correspondences, not
    // welded manifold half-edges: forcing opposite interior sides reflects
    // the complete target panel and repeats one half of the artwork.
    let source = flat_panel(0, "source", 0., 0., 1., seed_uv);
    let neighbor = flat_panel(2, "neighbor", 1.003, 0., 1., foreign_uv);
    // Deliberately reference the source's left edge and the target's left
    // edge. Endpoint projection retains their actual 1003 mm displacement;
    // both interiors must continue toward +X, regardless of edge ordering.
    let mut connection = join(&source, &neighbor, 1003.);
    connection.from.triangle = source[0].id.clone();
    connection.from.edge = 2;
    connection.reversed = false;
    let triangles = [source, neighbor].concat();
    let topology = Topology::new(triangles.clone(), &[connection]);
    for (part, center) in [
        ("source", Vec2::new(700., 450.)),
        ("neighbor", Vec2::new(1300., 450.)),
    ] {
        let chart = if part == "source" {
            seed_uv
        } else {
            foreign_uv
        };
        let rect = CoverageRect {
            center: chart(center),
            size: Vec2::new(9000., 9000.),
            rotation_rad: 0.19,
        };
        let actual = topology.coverage(part, rect, true);
        assert!(actual.iter().any(|f| f.part_id != part));
        assert_matches_sheet(&triangles, &actual, rect, |w| {
            chart(Vec2::new(w[0], w[1]) * 1000.)
        });
    }
}

#[test]
fn independently_reversed_mesh_winding_does_not_mirror_the_neighbor() {
    for invert_source in [false, true] {
        for invert_neighbor in [false, true] {
            let mut source = flat_panel(0, "source", 0., 0., 1., seed_uv);
            let mut neighbor = flat_panel(2, "neighbor", 1.003, 0., 1., foreign_uv);
            for (triangles, invert) in [
                (&mut source, invert_source),
                (&mut neighbor, invert_neighbor),
            ] {
                if invert {
                    for triangle in triangles {
                        triangle.world.swap(1, 2);
                        triangle.paint_uv.swap(1, 2);
                        triangle.source_barycentric.swap(1, 2);
                    }
                }
            }
            let mut connection = join(&source, &neighbor, 3.);
            if invert_source {
                connection.from.edge = 2;
            }
            if invert_neighbor {
                connection.to.edge = 0;
            }
            connection.reversed = invert_source == invert_neighbor;
            let triangles = [source, neighbor].concat();
            let topology = Topology::new(triangles.clone(), &[connection]);
            let rect = CoverageRect {
                center: seed_uv(Vec2::new(850., 450.)),
                size: Vec2::new(3000., 2000.),
                rotation_rad: 0.23,
            };
            let actual = topology.coverage("source", rect, true);
            assert_matches_sheet(&triangles, &actual, rect, |w| {
                seed_uv(Vec2::new(w[0], w[1]) * 1000.)
            });
        }
    }
}

#[test]
fn obtuse_body_fold_preserves_sheet_orientation_instead_of_projecting_a_mirror() {
    for angle in [100_f32, 120., 150.] {
        let source = flat_panel(0, "source", 0., 0., 1., |p| p);
        let direction = Vec3::new(angle.to_radians().cos(), 0., angle.to_radians().sin());
        let hinge = Vec3::new(1.003, 0., 0.);
        let neighbor = panel(
            2,
            "folded",
            [
                hinge,
                hinge + direction,
                hinge + Vec3::Y,
                hinge + Vec3::Y + direction,
            ]
            .map(|p| p.to_array()),
            [[0., 0.], [1000., 0.], [0., 1000.], [1000., 1000.]],
        );
        let connection = join(&source, &neighbor, 3.);
        let triangles = [source, neighbor].concat();
        let topology = Topology::new(triangles.clone(), &[connection]);
        let rect = CoverageRect {
            center: Vec2::new(850., 450.),
            size: Vec2::new(2600., 1600.),
            rotation_rad: 0.13,
        };
        let actual = topology.coverage("source", rect, true);
        assert_matches_sheet(&triangles, &actual, rect, |w| {
            let p = Vec3::from_array(w);
            Vec2::new(
                if p.z > 1e-6 {
                    1.003 + (p - hinge).dot(direction)
                } else {
                    p.x
                },
                p.y,
            ) * 1000.
        });
    }
}

#[test]
fn drag_transport_removes_chart_rotation_and_reflection_and_round_trips() {
    for rotation in [0_f32, 0.7, 1.5707963, -2.1] {
        for mirror in [1_f32, -1.] {
            let packing = Mat2::from_angle(rotation) * Mat2::from_diagonal(Vec2::new(mirror, 1.));
            let source = flat_panel(0, "door", 0., 0., 1., |p| p);
            let target = flat_panel(2, "hood", 0., 0., 1., |p| packing * p + Vec2::splat(2000.));
            let mut triangles = source;
            triangles.extend(target);
            let topology = Topology::new(triangles, &[]);
            let at = Vec2::new(200., 300.);
            let to = packing * at + Vec2::splat(2000.);
            for angle in [0_f32, 0.31, -1.2, 2.4] {
                let (result, flip) = topology
                    .transport_orientation("door", at, "hood", to, angle)
                    .unwrap();
                let y = packing.inverse() * Vec2::new(-result.sin(), result.cos());
                assert!(y.normalize().dot(Vec2::new(-angle.sin(), angle.cos())) > 0.99999);
                let x = packing.inverse()
                    * Vec2::new(result.cos(), result.sin())
                    * if flip { -1. } else { 1. };
                assert!(x.normalize().dot(Vec2::new(angle.cos(), angle.sin())) > 0.99999);
                let (back, undo_flip) = topology
                    .transport_orientation("hood", to, "door", at, result)
                    .unwrap();
                assert!((back - angle).cos() > 0.99999);
                assert_eq!(flip, undo_flip);
            }
        }
    }
}

#[test]
fn drag_transport_turns_with_surface_normal_not_packed_uv() {
    let mut triangles = panel(
        0,
        "door",
        [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
        [[0., 0.], [1000., 0.], [0., 1000.], [1000., 1000.]],
    );
    triangles.extend(panel(
        2,
        "hood",
        [[0., 0., 0.], [1., 0., 0.], [0., 0., -1.], [1., 0., -1.]],
        [[1000., 0.], [1000., 1000.], [0., 0.], [0., 1000.]],
    ));
    let topology = Topology::new(triangles, &[]);
    let (angle, flip) = topology
        .transport_orientation(
            "door",
            Vec2::new(200., 300.),
            "hood",
            Vec2::new(700., 200.),
            0.,
        )
        .unwrap();
    // Exterior normals of this wedge are +Z and -Y; +Y transports to +Z.
    assert!((angle + std::f32::consts::FRAC_PI_2).cos() > 0.99999);
    assert!(flip);
    assert!(
        topology
            .transport_orientation("missing", Vec2::ZERO, "hood", Vec2::ZERO, 0.)
            .is_none()
    );
}
