//! Real source faces whose rounded subdivision edges formerly left visible
//! coplanar islands outside the public runtime propagation graph.
mod support;

use glam::{DVec3, Vec2};
use itasha_surface_map::{PaintTriangle, topology::Topology};
use std::collections::BTreeSet;

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn area(t: &PaintTriangle) -> f64 {
    let w = world(t);
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6
}
fn centroid(t: &PaintTriangle) -> Vec2 {
    t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.
}
fn world_centroid(t: &PaintTriangle) -> DVec3 {
    world(t).into_iter().sum::<DVec3>() / 3.
}
fn check_face(face: u32) {
    let map = support::runtime_map();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let triangles = map
        .triangles
        .iter()
        .filter(|t| {
            t.id.source_mesh == "body.glb/node/91/primitive/0"
                && t.id.primitive == 0
                && t.id.source_triangle == face
                && t.part_id == "su7.trunk"
        })
        .collect::<Vec<_>>();
    let seed = *triangles
        .iter()
        .max_by(|a, b| area(a).total_cmp(&area(b)))
        .expect("reviewed original trunk face must remain");
    assert!(
        area(seed) > 1.,
        "reviewed source face must retain a stable seed"
    );
    let located = topology
        .locate(&seed.part_id, centroid(seed))
        .expect("stable source centroid must locate");
    assert_eq!(topology.triangles[located.0].id, seed.id);
    let coverage = topology.coverage_at(
        &seed.part_id,
        centroid(seed).to_array(),
        [300., 300.],
        0.,
        false,
    );
    let actual = coverage
        .iter()
        .map(|f| f.triangle.stable_key())
        .collect::<BTreeSet<_>>();
    let expected = triangles
        .iter()
        .filter(|t| {
            area(t) > 0.01 && world_centroid(t).distance(world_centroid(seed)) * 1000. < 100.
        })
        .collect::<Vec<_>>();
    assert!(
        expected.len() > 1,
        "regression face must exercise multiple substantial fragments"
    );
    let missing = expected
        .iter()
        .filter(|t| !actual.contains(&t.id.stable_key()))
        .map(|t| (t.id.stable_key(), area(t)))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "300 mm artwork from stable source face {face} must reach every substantial nearby coplanar fragment; missing {missing:?}"
    );
    eprintln!(
        "source face {face}: {} substantial fragments reached from stable seed",
        expected.len()
    );
}

#[test]
fn source_face_2779_subdivisions_remain_connected() {
    check_face(2779);
}

#[test]
fn source_face_2860_subdivisions_remain_connected() {
    check_face(2860);
}
