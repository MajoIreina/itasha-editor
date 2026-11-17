//! Original GLB shared-edge witnesses, independently classified from indices
//! and exact POSITION endpoints. Reviewed masks remain unchanged across gaps.
mod support;

use glam::{DMat2, DMat3, DQuat, DVec2, DVec3, Vec2};
use itasha_surface_map::{CoverageFragment, PaintTriangle, topology::Topology};

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn area(w: [DVec3; 3]) -> f64 {
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6
}
fn center(t: &PaintTriangle) -> Vec2 {
    t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.
}
fn original_normal(t: &PaintTriangle) -> DVec3 {
    let b = t.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        b / b.element_sum()
    });
    let w = world(t);
    let source = DMat3::from_cols(w[0], w[1], w[2]) * DMat3::from_cols(b[0], b[1], b[2]).inverse();
    (source.y_axis - source.x_axis)
        .cross(source.z_axis - source.x_axis)
        .normalize()
}
fn output_axes(f: &CoverageFragment) -> Option<([DVec3; 2], f64)> {
    let w: [DVec3; 3] = std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
    let area = area(w);
    if area < 0.1 {
        return None;
    }
    let q: [DVec2; 3] = std::array::from_fn(|i| Vec2::from_array(f.image_uv[i]).as_dvec2() * 300.);
    let j = DMat2::from_cols(q[1] - q[0], q[2] - q[0]);
    if j.determinant().abs() < 1e-10 {
        return None;
    }
    let inv = j.inverse();
    let axis = |v: DVec2| ((w[1] - w[0]) * v.x + (w[2] - w[0]) * v.y) * 1000.;
    Some(([axis(inv.x_axis), axis(inv.y_axis)], area))
}
fn check_pair(mesh: &str, part: &str, faces: [u32; 2]) {
    let map = support::runtime_map();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let stable = |face: u32| {
        map.triangles
            .iter()
            .filter(|t| {
                t.id.source_mesh == mesh
                    && t.id.primitive == 0
                    && t.id.source_triangle == face
                    && t.part_id == part
            })
            .max_by(|a, b| area(world(a)).total_cmp(&area(world(b))))
            .expect("reviewed original GLB face must remain")
    };
    for (a, b) in [(faces[0], faces[1]), (faces[1], faces[0])] {
        let from = stable(a);
        let target = stable(b);
        assert!(
            area(world(from)) > 0.1 && area(world(target)) > 0.1,
            "shared-edge witnesses require stable fragments"
        );
        let at = center(from);
        let located = topology
            .locate(part, at)
            .expect("shared-edge stable anchor must locate");
        assert_eq!(topology.triangles[located.0].id, from.id);
        let (_, u, v) = topology
            .tangent_frame(part, at)
            .expect("source anchor must have a physical frame");
        let normal = original_normal(from);
        let target_normal = original_normal(target);
        assert!(
            normal.dot(target_normal) > 0.8,
            "independently reviewed original GLB faces form an exterior bend under 37 degrees"
        );
        let turn = DQuat::from_rotation_arc(normal, target_normal);
        let expected = [turn * u.as_dvec3(), turn * v.as_dvec3()];
        let coverage = topology.coverage_at(part, at.to_array(), [300., 300.], 0., false);
        let measured=coverage.iter().filter(|f|f.triangle.source_mesh==mesh&&f.triangle.primitive==0&&f.triangle.source_triangle==b).filter_map(output_axes).max_by(|a,b|a.1.total_cmp(&b.1)).unwrap_or_else(||panic!("300 mm artwork from {mesh} source face {a} must cross the genuine original GLB edge into reviewed source face {b}"));
        let angles: [f64; 2] = std::array::from_fn(|i| {
            measured.0[i]
                .normalize()
                .dot(expected[i].normalize())
                .clamp(-1., 1.)
                .acos()
                .to_degrees()
        });
        assert!(
            angles.iter().all(|&angle| angle <= 1.),
            "original edge {a}->{b} must retain independently transported image axes, observed {angles:?} degrees"
        );
        for axis in measured.0 {
            assert!(
                (axis.length() - 1.).abs() < 0.01,
                "original-edge transport must retain physical image millimetres"
            );
        }
        eprintln!(
            "original edge {a}->{b}: {:.6} mm2 measured, image-axis errors {angles:?} degrees",
            measured.1
        );
    }
}

#[test]
fn original_trunk_edge_2856_3033_carries_physical_image_axes() {
    check_pair("body.glb/node/91/primitive/0", "su7.trunk", [2856, 3033]);
}
#[test]
fn original_rear_right_door_position_edge_3947_4503_carries_physical_image_axes() {
    check_pair(
        "body.glb/node/60/primitive/0",
        "su7.rear-right-door",
        [3947, 4503],
    );
}
#[test]
fn original_rear_right_door_position_edge_3858_4416_carries_physical_image_axes() {
    check_pair(
        "body.glb/node/60/primitive/0",
        "su7.rear-right-door",
        [3858, 4416],
    );
}
