//! Replay old public T queries through normalized original-source domains.
//! Usage: [--public-rounded] current-map old-map old-public-coverage-report output-report
use glam::{DMat2, DVec2, DVec3, Vec2};
use itasha_surface_map::{PaintSurfaceMap, PaintTriangle, barycentric_2d, topology::Topology};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

type SourceKey = (String, u32, u32, String);

fn source_key(t: &PaintTriangle) -> SourceKey {
    (
        t.id.source_mesh.clone(),
        t.id.primitive,
        t.id.source_triangle,
        t.part_id.clone(),
    )
}
fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn source(t: &PaintTriangle) -> [DVec2; 3] {
    t.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        DVec2::new(b.y, b.z) / b.element_sum()
    })
}
fn centroid(w: [DVec3; 3]) -> DVec3 {
    (w[0] + w[1] + w[2]) / 3.
}
fn mix(w: [DVec3; 3], b: DVec3) -> DVec3 {
    w[0] * b.x + w[1] * b.y + w[2] * b.z
}
fn source_area(t: &PaintTriangle) -> f64 {
    let s = source(t);
    (s[1] - s[0]).perp_dot(s[2] - s[0]).abs()
}
fn stable(t: &PaintTriangle) -> bool {
    let w = world(t);
    let longest = (w[1] - w[0])
        .length()
        .max((w[2] - w[0]).length())
        .max((w[2] - w[1]).length());
    let height = (w[1] - w[0]).cross(w[2] - w[0]).length() / longest;
    height.is_finite() && height > 2e-6 && source_area(t) > 0.
}
fn same_domain(a: &PaintTriangle, b: &PaintTriangle) -> bool {
    let mut a = source(a).map(|p| p.to_array().map(f64::to_bits));
    let mut b = source(b).map(|p| p.to_array().map(f64::to_bits));
    a.sort();
    b.sort();
    a == b
}

struct Remap {
    index: usize,
    mode: &'static str,
    bary: DVec3,
    point: Vec2,
    current_world: DVec3,
    old_world: DVec3,
    old_source_center: DVec2,
    old_source_point: DVec2,
    old_weights: DVec3,
    old_public_point: Option<Vec2>,
    containing: usize,
}
impl Remap {
    fn strict(&self) -> bool {
        self.mode == "old-source-centroid-contained"
            || self.mode == "old-public-rounded-sourcepoint-contained"
    }
    fn describe(
        &self,
        old: &PaintTriangle,
        current: &PaintSurfaceMap,
        topology: &Topology,
    ) -> Value {
        let t = &current.triangles[self.index];
        let located = topology.locate(&t.part_id, self.point);
        let located_world = located.map(|(i, b)| mix(world(&current.triangles[i]), b.as_dvec3()));
        let tangent = topology.tangent_frame(&t.part_id, self.point);
        let old_centroid = centroid(world(old));
        json!({
            "oldTriangle":old.id.stable_key(),"currentTriangle":t.id.stable_key(),
            "mode":self.mode,"strictOldSourcePointReplay":self.strict(),
            "strictOldSourceCentroidReplay":self.old_public_point.is_none().then(||self.strict()),
            "fragmentIdChanged":old.id!=t.id,"exactNormalizedSourceDomainUnchanged":same_domain(old,t),
            "partId":t.part_id,"sourceMesh":t.id.source_mesh,"primitive":t.id.primitive,"sourceTriangle":t.id.source_triangle,
            "oldNormalizedSourceCentroid":self.old_source_center.to_array(),
            "oldNormalizedSourcePoint":self.old_source_point.to_array(),
            "oldFragmentWeightsF64":self.old_weights.to_array(),
            "oldHistoricalPublicPaintPointF32":self.old_public_point.map(|p|p.to_array()),
            "currentBarycentricF64":self.bary.to_array(),"currentPaintPointF32":self.point.to_array(),
            "containingCurrentFragments":self.containing,
            "oldWorldCentroid":old_centroid.to_array(),"oldWorldPoint":self.old_world.to_array(),"currentWorldPoint":self.current_world.to_array(),
            "worldDistanceToOldPointMm":self.current_world.distance(self.old_world)*1000.,
            "worldDistanceToOldCentroidMm":self.current_world.distance(old_centroid)*1000.,
            "publicLocatedTriangle":located.map(|(i,_)|current.triangles[i].id.stable_key()),
            "publicLocateMatchesRemappedFragment":located.is_some_and(|(i,_)|i==self.index),
            "publicLocatedSameOriginalSourceAndPart":located.is_some_and(|(i,_)|source_key(&current.triangles[i])==source_key(old)),
            "publicRawWorldDistanceToOldPointMm":located_world.map(|w|w.distance(self.old_world)*1000.),
            "publicPhysicalWorldDistanceToOldPointMm":tangent.map(|(w,_,_)|w.as_dvec3().distance(self.old_world)*1000.),
            "publicRawWorldDistanceToOldCentroidMm":located_world.map(|w|w.distance(old_centroid)*1000.),
            "publicPhysicalWorldDistanceToOldCentroidMm":tangent.map(|(w,_,_)|w.as_dvec3().distance(old_centroid)*1000.),
        })
    }
}

fn remap(
    old: &PaintTriangle,
    current: &PaintSurfaceMap,
    groups: &HashMap<SourceKey, Vec<usize>>,
    old_public_point: Option<Vec2>,
) -> Result<Remap, &'static str> {
    let s = source(old);
    let old_weights = if let Some(point) = old_public_point {
        barycentric_2d(point, old.paint_uv.map(Vec2::from_array))
            .filter(|b| b.is_finite())
            .ok_or("old-public-paint-point-not-invertible-in-named-fragment")?
            .as_dvec3()
    } else {
        DVec3::splat(1. / 3.)
    };
    let old_source_center = (s[0] + s[1] + s[2]) / 3.;
    // Preserve the public f32 barycentrics exactly after promotion, without
    // normalizing or replacing them with the barycentrics of a located neighbor.
    let at = if old_public_point.is_some() {
        s[0] * old_weights.x + s[1] * old_weights.y + s[2] * old_weights.z
    } else {
        old_source_center
    };
    let members = groups
        .get(&source_key(old))
        .ok_or("no-current-same-original-source-and-part-fragments")?;
    let mut best: Option<(usize, DVec3, f64)> = None;
    let mut containing = 0;
    for &i in members {
        let p = source(&current.triangles[i]);
        let m = DMat2::from_cols(p[1] - p[0], p[2] - p[0]);
        if !m.determinant().is_finite() || m.determinant() == 0. {
            continue;
        }
        let q = m.inverse() * (at - p[0]);
        let b = DVec3::new(1. - q.x - q.y, q.x, q.y);
        // No containment epsilon: a fallback must remain visible in the report.
        if !b.is_finite() || b.min_element() < 0. {
            continue;
        }
        containing += 1;
        if best.as_ref().is_none_or(|&(j, _, score)| {
            b.min_element() > score
                || (b.min_element() == score
                    && current.triangles[i].id == old.id
                    && current.triangles[j].id != old.id)
        }) {
            best = Some((i, b, b.min_element()));
        }
    }
    let (index, bary, mode) = if let Some((i, b, _)) = best {
        (
            i,
            b,
            if old_public_point.is_some() {
                "old-public-rounded-sourcepoint-contained"
            } else {
                "old-source-centroid-contained"
            },
        )
    } else {
        let i = members
            .iter()
            .copied()
            .filter(|&i| stable(&current.triangles[i]))
            .max_by(|&a, &b| {
                source_area(&current.triangles[a]).total_cmp(&source_area(&current.triangles[b]))
            })
            .ok_or("old-source-point-outside-current-domains-and-no-stable-fallback")?;
        (
            i,
            DVec3::splat(1. / 3.),
            "fallback-largest-stable-same-source-fragment-centroid",
        )
    };
    let t = &current.triangles[index];
    let uv = t.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
    let point = (uv[0] * bary.x + uv[1] * bary.y + uv[2] * bary.z).as_vec2();
    Ok(Remap {
        index,
        mode,
        bary,
        point,
        current_world: mix(world(t), bary),
        old_world: if old_public_point.is_some() {
            mix(world(old), old_weights)
        } else {
            centroid(world(old))
        },
        old_source_center,
        old_source_point: at,
        old_weights,
        old_public_point,
        containing,
    })
}

fn historical_point(query: &Value, key: &str) -> Result<Vec2, Box<dyn std::error::Error>> {
    let p: [f32; 2] = serde_json::from_value(query[key].clone())?;
    let p = Vec2::from_array(p);
    if !p.is_finite() {
        return Err(format!("non-finite historical {key}").into());
    }
    Ok(p)
}

fn old_public_locate(
    expected: &PaintTriangle,
    point: Vec2,
    old: &PaintSurfaceMap,
    topology: &Topology,
) -> Value {
    let located = topology.locate(&expected.part_id, point);
    let named_bary = barycentric_2d(point, expected.paint_uv.map(Vec2::from_array));
    json!({
        "paintPointF32":point.to_array(),"expectedTriangle":expected.id.stable_key(),
        "namedFragmentBarycentricF32":named_bary.map(|b|b.to_array()),
        "namedFragmentBarycentricNonnegative":named_bary.is_some_and(|b|b.is_finite()&&b.min_element()>=0.),
        "locatedTriangle":located.map(|(i,_)|old.triangles[i].id.stable_key()),
        "locatedBarycentricF32":located.map(|(_,b)|b.to_array()),
        "matchesExpectedFragment":located.is_some_and(|(i,_)|old.triangles[i].id==expected.id),
        "sameOriginalSourceAndPart":located.is_some_and(|(i,_)|source_key(&old.triangles[i])==source_key(expected)),
        "definition":"The old public locate result is recorded separately. Replay always uses the named old fragment's paint-UV inversion; a located neighbor is never substituted."
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let public_rounded = args.first().is_some_and(|arg| arg == "--public-rounded");
    if public_rounded {
        args.remove(0);
    }
    if args.len() != 4 {
        return Err("usage: [--public-rounded] current-map old-map old-public-coverage-report output-report".into());
    }
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let old: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let historical: Value = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    map.validate()?;
    old.validate()?;
    if map.model_fingerprint != old.model_fingerprint {
        return Err("original model changed".into());
    }
    if historical["paintFingerprint"].as_str() != Some(old.paint_fingerprint.as_str()) {
        return Err("historical report fingerprint differs from old map".into());
    }
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let old_topology = Topology::new(old.triangles.clone(), &old.connections);
    let old_ids: HashMap<_, _> = old
        .triangles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.stable_key(), i))
        .collect();
    let mut groups: HashMap<SourceKey, Vec<usize>> = HashMap::new();
    for (i, t) in map.triangles.iter().enumerate() {
        groups.entry(source_key(t)).or_default().push(i);
    }
    let mut rows = Vec::new();
    for (ordinal, query) in historical["results"]
        .as_array()
        .ok_or("old report has no public query results")?
        .iter()
        .enumerate()
    {
        let from = query["from"].as_str().ok_or("missing old from ID")?;
        let to = query["to"].as_str().ok_or("missing old to ID")?;
        let src = &old.triangles[*old_ids.get(from).ok_or("old source fragment absent")?];
        let dst = &old.triangles[*old_ids.get(to).ok_or("old target fragment absent")?];
        let old_source_point = historical_point(query, "sourceCenter")?;
        let old_target_point = historical_point(query, "targetCenter")?;
        let old_source_locate = old_public_locate(src, old_source_point, &old, &old_topology);
        let old_target_locate = old_public_locate(dst, old_target_point, &old, &old_topology);
        let a = remap(
            src,
            &map,
            &groups,
            public_rounded.then_some(old_source_point),
        );
        let b = remap(
            dst,
            &map,
            &groups,
            public_rounded.then_some(old_target_point),
        );
        let (Ok(a), Ok(b)) = (&a, &b) else {
            rows.push(json!({"ordinal":ordinal,"partId":src.part_id,"oldFrom":from,"oldTo":to,"unmapped":true,"strictHistoricalReplay":false,
                "sourceUnmappedReason":a.err(),"targetUnmappedReason":b.err(),
                "oldSourcePublicLocate":old_source_locate,"oldTargetPublicLocate":old_target_locate,
                "oldTargetMissing":query["targetFragments"]==0,"oldWitnessQuery":query}));
            continue;
        };
        let target = &map.triangles[b.index];
        let coverage =
            topology.coverage_at(&src.part_id, a.point.to_array(), [300., 300.], 0., true);
        let hits: Vec<_> = coverage
            .iter()
            .filter(|f| f.triangle == target.id)
            .collect();
        let target_point_covered = hits.iter().any(|f| {
            f.paint_uv.len() == 3
                && barycentric_2d(
                    b.point,
                    [f.paint_uv[0], f.paint_uv[1], f.paint_uv[2]].map(Vec2::from_array),
                )
                .is_some_and(|b| b.min_element() >= -1e-5)
        });
        rows.push(json!({
            "ordinal":ordinal,"partId":src.part_id,"oldFrom":from,"oldTo":to,"unmapped":false,
            "oldSourcePublicLocate":old_source_locate,"oldTargetPublicLocate":old_target_locate,
            "strictHistoricalReplay":a.strict()&&b.strict(),"fromRemap":a.describe(src,&map,&topology),"targetRemap":b.describe(dst,&map,&topology),
            "coverageFragments":coverage.len(),"targetFragments":hits.len(),"targetMissing":hits.is_empty(),
            "targetHistoricalPointCoveredInPublicPaintUv":target_point_covered,
            "oldTargetFragments":query["targetFragments"],"oldTargetMissing":query["targetFragments"]==0,
            "oldWitnessQuery":query,
        }));
    }
    let count = |f: fn(&Value) -> bool| rows.iter().filter(|r| f(r)).count();
    let by_part:BTreeMap<_,_>=rows.iter().map(|r|r["partId"].as_str().unwrap()).map(|part|{
        let rs:Vec<_>=rows.iter().filter(|r|r["partId"]==part).collect();
        (part,json!({"queries":rs.len(),"targetMissing":rs.iter().filter(|r|r["targetMissing"]==true).count(),"fallbackQueries":rs.iter().filter(|r|r["unmapped"]!=true&&r["strictHistoricalReplay"]!=true).count(),"unmapped":rs.iter().filter(|r|r["unmapped"]==true).count()}))
    }).collect();
    let summary = json!({
        "queries":rows.len(),"executedPublicCoverageQueries":count(|r|r["unmapped"]!=true),"unmappedQueries":count(|r|r["unmapped"]==true),
        "strictSourceDomainQueries":count(|r|r["strictHistoricalReplay"]==true),"fallbackQueries":count(|r|r["unmapped"]!=true&&r["strictHistoricalReplay"]!=true),
        "sourceFallbacks":count(|r|r["fromRemap"]["strictOldSourcePointReplay"]==false),"targetFallbacks":count(|r|r["targetRemap"]["strictOldSourcePointReplay"]==false),
        "changedSourceFragmentIds":count(|r|r["fromRemap"]["fragmentIdChanged"]==true),"changedTargetFragmentIds":count(|r|r["targetRemap"]["fragmentIdChanged"]==true),
        "changedSourceDomains":count(|r|r["fromRemap"]["exactNormalizedSourceDomainUnchanged"]==false),"changedTargetDomains":count(|r|r["targetRemap"]["exactNormalizedSourceDomainUnchanged"]==false),
        "targetMissing":count(|r|r["targetMissing"]==true),"strictSourceDomainTargetMissing":count(|r|r["strictHistoricalReplay"]==true&&r["targetMissing"]==true),
        "targetHistoricalPointNotCovered":count(|r|r["unmapped"]!=true&&r["targetHistoricalPointCoveredInPublicPaintUv"]!=true),
        "publicSourceLocateDifferentFragment":count(|r|r["fromRemap"]["publicLocateMatchesRemappedFragment"]==false),
        "publicSourceLocateDifferentOriginalSourceOrPart":count(|r|r["fromRemap"]["publicLocatedSameOriginalSourceAndPart"]==false),
        "oldPublicSourceLocateDifferentNamedFragment":count(|r|r["oldSourcePublicLocate"]["matchesExpectedFragment"]==false),
        "oldPublicTargetLocateDifferentNamedFragment":count(|r|r["oldTargetPublicLocate"]["matchesExpectedFragment"]==false),
        "oldPublicSourceLocateDifferentOriginalSourceOrPart":count(|r|r["oldSourcePublicLocate"]["sameOriginalSourceAndPart"]==false),
        "oldPublicTargetLocateDifferentOriginalSourceOrPart":count(|r|r["oldTargetPublicLocate"]["sameOriginalSourceAndPart"]==false),
        "oldTargetMissing":count(|r|r["oldTargetMissing"]==true),
    });
    let old_point_mode = if public_rounded {
        "public-rounded"
    } else {
        "mathematical-source-centroid"
    };
    let definition = if public_rounded {
        "Every old public query is retained. Read sourceCenter/targetCenter as f32, invert each paint point in its named old fragment with the public f32 barycentric_2d, promote those exact f32 weights to f64, and multiply the old normalized stored-f32 source corners in f64. Do not renormalize those public weights or substitute the old public located fragment. Map that point to a current same-source-face, same-part retained triangle with strictly nonnegative f64 barycentrics; prefer largest minimum barycentric. No containment epsilon. If absent, use largest stable same-source fragment centroid as explicit fallback, never a strict historical replay. Uninvertible old points remain unmapped. The old public locate identity is recorded separately. Current public f32 locate and 300x300mm rotation-zero adjacent coverage are actually executed. Target fragment presence and remapped old target point public paint-UV containment are separate."
    } else {
        "Every exact old public query is retained. Normalize each old fragment's source barycentric corners in f64 and map its mathematical source centroid to a current same-source-face, same-part retained triangle with strictly nonnegative f64 barycentrics; prefer largest minimum barycentric. No containment epsilon. If absent, use largest stable same-source fragment centroid as explicit fallback, never a strict historical replay. Public f32 locate and 300x300mm rotation-zero adjacent coverage are actually executed. Target fragment presence and the remapped old target point's public paint-UV containment are reported separately. Regenerated fragment IDs do not imply unchanged target domains. Old historical public locate identities are recorded separately without changing this mathematical-centroid replay."
    };
    let report = json!({"schema":"itasha-historical-t-source-domain-replay-v1","currentMap":args[0],"oldMap":args[1],"oldPublicReport":args[2],"paintFingerprint":map.paint_fingerprint,"oldPaintFingerprint":old.paint_fingerprint,
        "oldPointMode":old_point_mode,"definition":definition,"summary":summary,"byPart":by_part,"results":rows});
    std::fs::write(&args[3], serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report["summary"])?);
    println!("saved {}", args[3]);
    Ok(())
}
