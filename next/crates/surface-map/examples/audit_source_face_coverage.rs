//! Independent public coverage scan of substantial same-source-face domains.
use glam::{DVec3, Vec2};
use itasha_surface_map::{PaintSurfaceMap, PaintTriangle, topology::Topology};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn area(t: &PaintTriangle) -> f64 {
    let w = world(t);
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6
}
fn center(t: &PaintTriangle) -> Vec2 {
    t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.
}
fn origin(t: &PaintTriangle) -> DVec3 {
    world(t).into_iter().sum::<DVec3>() / 3.
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let part = args.get(2).map(String::as_str).unwrap_or("su7.trunk");
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let mut groups: BTreeMap<(&str, &str, u32, u32), Vec<usize>> = BTreeMap::new();
    for (i, t) in map
        .triangles
        .iter()
        .enumerate()
        .filter(|(_, t)| part == "all" || t.part_id == part)
    {
        groups
            .entry((
                &t.part_id,
                &t.id.source_mesh,
                t.id.primitive,
                t.id.source_triangle,
            ))
            .or_default()
            .push(i);
    }
    let start = Instant::now();
    let mut rows = Vec::new();
    let mut skipped_single = 0;
    let mut skipped_unstable = 0;
    let mut eligible_targets = 0;
    let mut missing_targets = 0;
    let mut missing_area = 0.;
    let mut seed_lookup_mismatch = 0;
    for ((part, mesh, primitive, source_face), indices) in &groups {
        if indices.len() < 2 {
            skipped_single += 1;
            continue;
        }
        let &seed = indices
            .iter()
            .max_by(|&&a, &&b| area(&map.triangles[a]).total_cmp(&area(&map.triangles[b])))
            .unwrap();
        let t = &map.triangles[seed];
        if area(t) <= 1. {
            skipped_unstable += 1;
            continue;
        }
        let at = center(t);
        let locate = topology.locate(part, at);
        let own = locate.is_some_and(|(i, _)| i == seed);
        if !own {
            seed_lookup_mismatch += 1;
        }
        let coverage = topology.coverage_at(part, at.to_array(), [300., 300.], 0., false);
        let actual = coverage
            .iter()
            .map(|f| &f.triangle)
            .collect::<BTreeSet<_>>();
        let w = world(t);
        let normal = (w[1] - w[0]).cross(w[2] - w[0]).normalize();
        let expected = indices
            .iter()
            .filter(|&&i| {
                area(&map.triangles[i]) > 0.01
                    && origin(t).distance(origin(&map.triangles[i])) * 1000. < 100.
            })
            .copied()
            .collect::<Vec<_>>();
        eligible_targets += expected.len();
        let missing=expected.iter().filter(|&&i|!actual.contains(&map.triangles[i].id)).map(|&i| {
            let q=&map.triangles[i];let qw=world(q);
            let n=(qw[1]-qw[0]).cross(qw[2]-qw[0]).normalize();
            let located=topology.locate(part,center(q));
            json!({"triangle":q.id.stable_key(),"areaMm2":area(q),"chartId":q.chart_id,"centroidWorld":origin(q).to_array(),"centroidDistanceMm":origin(t).distance(origin(q))*1000.,"normalDot":normal.dot(n),"targetCentroidLocatesOwnTriangle":located.is_some_and(|(j,_)|j==i)})
        }).collect::<Vec<_>>();
        let sum_area = missing
            .iter()
            .map(|x| x["areaMm2"].as_f64().unwrap())
            .sum::<f64>();
        missing_targets += missing.len();
        missing_area += sum_area;
        rows.push(json!({"partId":part,"sourceMesh":mesh,"primitive":primitive,"sourceFace":source_face,"sourceFragments":indices.len(),"eligibleTargets":expected.len(),"missingTargets":missing.len(),"missingAreaMm2":sum_area,"seed":t.id.stable_key(),"seedAreaMm2":area(t),"seedChartId":t.chart_id,"seedCentroidLocatesOwnTriangle":own,"sourceCenter":at.to_array(),"seedWorld":origin(t).to_array(),"coverageFragments":coverage.len(),"missing":missing}));
        if rows.len() % 1000 == 0 {
            eprintln!(
                "scanned {} source faces in {:.1}s",
                rows.len(),
                start.elapsed().as_secs_f64()
            );
        }
    }
    rows.sort_by(|a, b| {
        b["missingAreaMm2"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["missingAreaMm2"].as_f64().unwrap())
    });
    let failed_faces = rows
        .iter()
        .filter(|r| r["missingTargets"].as_u64().unwrap() > 0)
        .count();
    let mut by_part: BTreeMap<&str, Value> = BTreeMap::new();
    for row in &rows {
        let entry = by_part.entry(row["partId"].as_str().unwrap()).or_insert(
            json!({"faces":0,"facesWithMissingTargets":0,"missingTargets":0,"missingAreaMm2":0.}),
        );
        for (key, increment) in [
            ("faces", 1),
            (
                "facesWithMissingTargets",
                u64::from(row["missingTargets"].as_u64().unwrap() > 0),
            ),
            ("missingTargets", row["missingTargets"].as_u64().unwrap()),
        ] {
            entry[key] = json!(entry[key].as_u64().unwrap() + increment);
        }
        entry["missingAreaMm2"] = json!(
            entry["missingAreaMm2"].as_f64().unwrap() + row["missingAreaMm2"].as_f64().unwrap()
        );
    }
    let report = json!({"map":args[0],"paintFingerprint":map.paint_fingerprint,"partFilter":part,"definition":"Every multi-fragment (part,sourceMesh,primitive,sourceFace) with largest stored f64-world-area seed >1mm2. Public centroid locate and 300x300mm same-part coverage. Expected targets have stored-world area >0.01mm2 and centroid distance <100mm. These thresholds select substantial witnesses; they do not discard smaller source domains from geometry. Missing target lists retain chart, own-centroid lookup, distance and normal evidence.","sourceGroups":groups.len(),"skippedSingleFragmentGroups":skipped_single,"skippedNoStableSeedGroups":skipped_unstable,"queriedSourceFaces":rows.len(),"facesWithMissingTargets":failed_faces,"eligibleTargets":eligible_targets,"missingTargets":missing_targets,"missingAreaMm2":missing_area,"seedLookupIdentityMismatches":seed_lookup_mismatch,"elapsedSeconds":start.elapsed().as_secs_f64(),"byPart":by_part,"results":rows});
    std::fs::write(&args[1], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "saved {}: {} faces, {} missing targets on {} faces",
        args[1], report["queriedSourceFaces"], missing_targets, failed_faces
    );
    Ok(())
}
