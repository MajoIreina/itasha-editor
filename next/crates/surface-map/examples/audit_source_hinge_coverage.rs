//! Independently differentiate image axes across every authored original edge.
use glam::{DMat2, DMat3, DQuat, DVec2, DVec3, Vec2};
use itasha_surface_map::{CoverageFragment, PaintSurfaceMap, PaintTriangle, topology::Topology};
use serde_json::json;
use std::collections::HashMap;

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn area(w: [DVec3; 3]) -> f64 {
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 5e5
}
fn original(t: &PaintTriangle) -> [DVec3; 3] {
    let b = t.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        b / b.element_sum()
    });
    let w = world(t);
    let a = DMat3::from_cols(w[0], w[1], w[2]) * DMat3::from_cols(b[0], b[1], b[2]).inverse();
    [a.x_axis, a.y_axis, a.z_axis]
}
fn axes(f: &CoverageFragment) -> Option<([DVec3; 2], f64)> {
    let w: [DVec3; 3] = std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
    let a = area(w);
    if a < 0.01 {
        return None;
    }
    let q: [DVec2; 3] =
        std::array::from_fn(|i| DVec2::from_array(f.image_uv[i].map(f64::from)) * 300.);
    let inverse = DMat2::from_cols(q[1] - q[0], q[2] - q[0]).inverse();
    if !inverse.is_finite() {
        return None;
    }
    let apply = |v: DVec2| ((w[1] - w[0]) * v.x + (w[2] - w[0]) * v.y) * 1000.;
    let a = [apply(inverse.x_axis), apply(inverse.y_axis)];
    a.iter().all(|v| v.is_finite()).then_some((a, area(w)))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    map.validate()?;
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let ids: HashMap<_, _> = map.triangles.iter().map(|t| (&t.id, t)).collect();
    let same_face = |a: &PaintTriangle, b: &PaintTriangle| {
        a.part_id == b.part_id
            && a.id.source_mesh == b.id.source_mesh
            && a.id.primitive == b.id.primitive
            && a.id.source_triangle == b.id.source_triangle
    };
    let stable = |t: &PaintTriangle| {
        map.triangles
            .iter()
            .filter(|q| same_face(t, q))
            .max_by(|a, b| area(world(a)).total_cmp(&area(world(b))))
            .unwrap()
    };
    let mut rows = Vec::new();
    for c in map.connections.iter().filter(|c| c.source_hinge.is_some()) {
        let h = c.source_hinge.as_ref().unwrap();
        let a = stable(ids[&c.from.triangle]);
        let b = stable(ids[&c.to.triangle]);
        let line = h.world.map(|p| DVec3::from_array(p.map(f64::from)));
        let direction = (line[1] - line[0]).normalize();
        let inward = |p: DVec3| {
            let d = p - line[0];
            (d - direction * d.dot(direction)).normalize()
        };
        let a_in = inward(original(a)[(h.from_source_edge as usize + 2) % 3]);
        let b_in = inward(original(b)[(h.to_source_edge as usize + 2) % 3]);
        let n_a = direction.cross(a_in).normalize();
        let n_b = direction.cross(-b_in).normalize();
        let turn =
            DQuat::from_axis_angle(direction, direction.dot(n_a.cross(n_b)).atan2(n_a.dot(n_b)));
        for (source, target, rotation, label) in
            [(a, b, turn, "from-to"), (b, a, turn.conjugate(), "to-from")]
        {
            let at = source
                .paint_uv
                .map(Vec2::from_array)
                .into_iter()
                .sum::<Vec2>()
                / 3.;
            let located = topology.locate(&source.part_id, at);
            let frame = topology.tangent_frame(&source.part_id, at);
            let coverage =
                topology.coverage_at(&source.part_id, at.to_array(), [300., 300.], 0., false);
            let target_output: Vec<_> = coverage
                .iter()
                .filter(|f| same_face(target, ids[&f.triangle]))
                .collect();
            let measured = target_output
                .iter()
                .filter_map(|f| axes(f).map(|(axes, area)| (*f, axes, area)))
                .max_by(|a, b| a.2.total_cmp(&b.2));
            let witness=frame.zip(measured).map(|((_,u,v),(f,measured,a))|{
                let expected=[rotation*u.as_dvec3().normalize(),rotation*v.as_dvec3().normalize()];
                let errors:[f64;2]=std::array::from_fn(|i|expected[i].dot(measured[i].normalize()).clamp(-1.,1.).acos().to_degrees());
                let lengths=measured.map(|a|a.length());
                json!({"targetOutputTriangle":f.triangle.stable_key(),"targetOutputAreaMm2":a,"axisErrorsDegrees":errors,"axisLengthsPerMm":lengths,"passes":errors.iter().all(|a|*a<=1.)&&lengths.iter().all(|a|(a-1.).abs()<=0.01)})
            });
            rows.push(json!({"hinge":c.id,"direction":label,"source":source.id.stable_key(),"target":target.id.stable_key(),"partId":source.part_id,"sourceCenter":at.to_array(),"sourceAreaMm2":area(world(source)),"targetAreaMm2":area(world(target)),"locatesSource":located.is_some_and(|(i,_)|topology.triangles[i].id==source.id),"sourceNormalTurnDegrees":n_a.dot(n_b).clamp(-1.,1.).acos().to_degrees(),"supportIntervals":h.support_intervals,"coverageFragments":coverage.len(),"targetFragments":target_output.len(),"witness":witness}));
        }
    }
    let summary = json!({"hinges":rows.len()/2,"queries":rows.len(),"targetMissing":rows.iter().filter(|r|r["targetFragments"]==0).count(),"sourceLocateMismatch":rows.iter().filter(|r|r["locatesSource"]!=true).count(),"unusable":rows.iter().filter(|r|r["witness"].is_null()).count(),"metricOrDirectionFailures":rows.iter().filter(|r|!r["witness"].is_null()&&r["witness"]["passes"]!=true).count()});
    let report = json!({"schema":"itasha-original-source-hinge-runtime-axes-audit-v1","map":args[0],"paintFingerprint":map.paint_fingerprint,"definition":"Every authored source hinge, both directions, 300mm square anchored on largest raw-world same-source-face/part fragment. Differentiate emitted imageUv*300mm against raw world millimetres; compare with public source tangent axes rotated independently around the original shared GLB line. Original interiors come from stable raw world/source-barycentric affine inverse, not production private frames. Hinge support remains the production gate; missing and numerically unusable witnesses remain explicit.","summary":summary,"results":rows});
    std::fs::write(&args[1], serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report["summary"])?);
    Ok(())
}
