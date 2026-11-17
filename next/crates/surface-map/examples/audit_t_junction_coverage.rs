//! Read-only public-runtime coverage witnesses for source-aware T candidates.
use glam::{DVec3, Vec2};
use itasha_surface_map::{PaintSurfaceMap, PaintTriangle, topology::Topology};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn center(t: &PaintTriangle) -> Vec2 {
    t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.
}
fn area(t: &PaintTriangle) -> f64 {
    let w = world(t);
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let source_report: Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let ids: BTreeMap<_, _> = map
        .triangles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.stable_key(), i))
        .collect();
    let mut pairs = BTreeMap::new();
    for t in source_report["anchors"]["tJunctions"]["longestExamples"]
        .as_array()
        .unwrap()
    {
        if t["bothComponentsAbove0_01Mm2"] != true {
            continue;
        }
        let a = ids[t["triangle"].as_str().unwrap()];
        let ar = t["component"].as_u64().unwrap();
        for hit in t["interiorBoundaryVertices"].as_array().unwrap() {
            if hit["separateComponent"] != true {
                continue;
            }
            let components = hit["incidentComponents"].as_array().unwrap();
            if !components.iter().any(|c| {
                c["component"].as_u64().unwrap() != ar && c["areaMm2"].as_f64().unwrap() > 0.01
            }) {
                continue;
            }
            for id in hit["incidentTriangles"].as_array().unwrap() {
                let b = ids[id.as_str().unwrap()];
                let key = (a.min(b), a.max(b));
                pairs.entry(key).or_insert((a, b, t.clone(), hit.clone()));
            }
        }
    }
    let mut rows = Vec::new();
    for (_, (a, b, edge, hit)) in pairs {
        for (a, b) in [(a, b), (b, a)] {
            let src = &map.triangles[a];
            let dst = &map.triangles[b];
            let at = center(src);
            let target_at = center(dst);
            let locate = topology.locate(&src.part_id, at);
            let target_locate = topology.locate(&dst.part_id, target_at);
            let coverage =
                topology.coverage_at(&src.part_id, at.to_array(), [300., 300.], 0., true);
            let target_hits = coverage.iter().filter(|f| f.triangle == dst.id).count();
            let from_own = locate.is_some_and(|(i, _)| i == a);
            let target_own = target_locate.is_some_and(|(i, _)| i == b);
            let aw = world(src);
            let bw = world(dst);
            let distance =
                ((aw[0] + aw[1] + aw[2]) / 3.).distance((bw[0] + bw[1] + bw[2]) / 3.) * 1000.;
            let normal_dot = (aw[1] - aw[0])
                .cross(aw[2] - aw[0])
                .normalize()
                .dot((bw[1] - bw[0]).cross(bw[2] - bw[0]).normalize());
            rows.push(json!({"partId":src.part_id,"from":src.id.stable_key(),"to":dst.id.stable_key(),"fromAreaMm2":area(src),"toAreaMm2":area(dst),"sourceCenter":at.to_array(),"targetCenter":target_at.to_array(),"centroidWorldDistanceMm":distance,"rawWorldNormalDot":normal_dot,"sameSourceFace":src.id.source_mesh==dst.id.source_mesh&&src.id.primitive==dst.id.primitive&&src.id.source_triangle==dst.id.source_triangle,"fromCentroidLocatesOwnTriangle":from_own,"targetCentroidLocatesOwnTriangle":target_own,"coverageFragments":coverage.len(),"targetFragments":target_hits,"edgeWitness":edge,"vertexWitness":hit}));
        }
    }
    let count = |f: fn(&Value) -> bool| rows.iter().filter(|r| f(r)).count();
    let same_face_missing = count(|r| {
        r["sameSourceFace"] == true
            && r["targetFragments"] == 0
            && r["fromCentroidLocatesOwnTriangle"] == true
            && r["targetCentroidLocatesOwnTriangle"] == true
    });
    let parts: BTreeSet<_> = rows.iter().map(|r| r["partId"].as_str().unwrap()).collect();
    let by_part: BTreeMap<_,_>=parts.iter().map(|p|(*p,json!({"queries":rows.iter().filter(|r|r["partId"]==*p).count(),"targetMissing":rows.iter().filter(|r|r["partId"]==*p&&r["targetFragments"]==0).count()}))).collect();
    let report = json!({"map":args[0],"sourceReport":args[1],"paintFingerprint":map.paint_fingerprint,"definition":"Each large-component geometric T edge and its incident triangles, both directions; public f32 centroid locate, 300x300mm coverage. Exact target fragment absence is evidence, not alone proof of visible exterior failure; inspect source identity, separation, normal, target area, and own-centroid lookup witnesses.","queries":rows.len(),"targetMissing":count(|r|r["targetFragments"]==0),"sameSourceFaceOwnCentroidTargetMissing":same_face_missing,"byPart":by_part,"results":rows});
    std::fs::write(&args[2], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "saved {}: {} queries, {} same-source-face own-centroid misses",
        args[2], report["queries"], same_face_missing
    );
    Ok(())
}
