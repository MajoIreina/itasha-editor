//! Verify a chart replacement against saved fragment anchors in its predecessor.
use glam::Vec3;
use itasha_surface_map::{
    PaintSurfaceMap,
    topology::{Topology, triangle_tangent_axes},
    transition::{MappingTransition, SourcePartCorrection},
};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 && !(args.len() == 4 && args[3] == "--su7-reviewed-part-correction") {
        return Err("expected predecessor.json replacement.json report.json [--su7-reviewed-part-correction]".into());
    }
    let old: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let new: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let corrections = [SourcePartCorrection {
        source_mesh: "body.glb/node/175/primitive/0",
        primitive: 0,
        source_triangle: 32590,
        from_part: "su7.rear-bumper",
        to_part: "su7.front-left-fender",
    }];
    let migration = MappingTransition::with_part_corrections(
        &old,
        &new,
        if args.len() == 4 { &corrections } else { &[] },
    )?;
    let topology = Topology::new(new.triangles.clone(), &new.connections);
    let targets: HashMap<_, _> = new.triangles.iter().map(|t| (&t.id, t)).collect();
    let mut counts: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    let mut failures = vec![];
    let mut rejection_reasons: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut max_world_mm = 0_f32;
    let mut max_angle = 0_f32;
    let mut worst_angles = vec![];
    let mut worst_positions = vec![];
    let mix = |w: [[f32; 3]; 3], b: Vec3| {
        let w = w.map(Vec3::from_array);
        w[0] * b.x + w[1] * b.y + w[2] * b.z
    };
    for t in &old.triangles {
        for bary in [
            Vec3::ONE / 3.,
            Vec3::new(0.8, 0.1, 0.1),
            Vec3::new(0.1, 0.8, 0.1),
            Vec3::new(0.1, 0.1, 0.8),
        ] {
            let count = counts.entry(t.part_id.clone()).or_default();
            match migration.reproject(&t.id.stable_key(), &t.part_id, bary, 0.47) {
                Ok(p) => {
                    count[0] += 1;
                    let target = targets[&p.triangle];
                    let world_error =
                        (mix(t.world, bary) - mix(target.world, p.barycentric)).length() * 1000.;
                    max_world_mm = max_world_mm.max(world_error);
                    let (u, v) = triangle_tangent_axes(t).ok_or("old tangent")?;
                    let (_, tu, tv) = topology
                        .tangent_frame(&p.part_id, p.paint_uv)
                        .ok_or("new runtime tangent")?;
                    let (tu, tv) = (tu.normalize(), tv.normalize());
                    let y = v * 0.47_f32.cos() - u * 0.47_f32.sin();
                    let ty = tv * p.rotation_rad.cos() - tu * p.rotation_rad.sin();
                    let angle_error = y.cross(ty).length().atan2(y.dot(ty)).to_degrees();
                    max_angle = max_angle.max(angle_error);
                    let area = |points: [[f32; 3]; 3]| {
                        let p = points.map(|p| Vec3::from_array(p).as_dvec3());
                        (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5 * 1e6
                    };
                    let row = json!({"part": t.part_id, "oldTriangle": t.id.stable_key(), "newTriangle": target.id.stable_key(), "oldBary": bary.to_array(), "newBary": p.barycentric.to_array(), "worldErrorMm": world_error, "angleErrorDeg": angle_error, "oldAreaMm2": area(t.world), "newAreaMm2": area(target.world), "oldAxes":[u.to_array(),v.to_array()], "newAxes":[tu.to_array(),tv.to_array()]});
                    for (rows, error) in [
                        (&mut worst_angles, angle_error),
                        (&mut worst_positions, world_error),
                    ] {
                        if rows.len() < 20
                            || error > rows.last().map_or(0., |r: &(f32, serde_json::Value)| r.0)
                        {
                            rows.push((error, row.clone()));
                            rows.sort_by(|a, b| b.0.total_cmp(&a.0));
                            rows.truncate(20);
                        }
                    }
                }
                Err(error) => {
                    count[1] += 1;
                    *rejection_reasons
                        .entry(t.part_id.clone())
                        .or_default()
                        .entry(error.clone())
                        .or_default() += 1;
                    if failures.len() < 100 {
                        failures.push(json!({"part":t.part_id,"triangle":t.id.stable_key(),"barycentric":bary.to_array(),"error":error}));
                    }
                }
            }
        }
    }
    let report = json!({"oldFingerprint":old.paint_fingerprint,"newFingerprint":new.paint_fingerprint,"probesPerFragment":4,"parts":counts,"maxWorldErrorMm":max_world_mm,"maxVerticalAngleErrorDeg":max_angle,"worstAngles":worst_angles.into_iter().map(|r|r.1).collect::<Vec<_>>(),"worstPositions":worst_positions.into_iter().map(|r|r.1).collect::<Vec<_>>(),"rejectedSamples":failures,"rejectionReasons":rejection_reasons,"rejectionPolicy":"Atomic document/history migration refuses any anchor outside the reviewed replacement mask or whose image axes cannot be preserved within one degree; never snaps or changes the source face."});
    std::fs::write(&args[2], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"report":args[2],"parts":report["parts"],"maxWorldErrorMm":max_world_mm,"maxVerticalAngleErrorDeg":max_angle})
        )?
    );
    Ok(())
}
