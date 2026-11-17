//! Public frame audit against independently decoded original GLB corners.
//! Usage: map original-source-reference prior-all-frame-report output-report
use glam::{DVec3, Vec2};
use itasha_surface_map::{PaintSurfaceMap, topology::Topology};
use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "usage: map original-source-reference prior-all-frame-report output-report".into(),
        );
    }
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    map.validate()?;
    let reference: Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let prior: Value = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    if reference["paintFingerprint"] != map.paint_fingerprint
        || prior["paintFingerprint"] != map.paint_fingerprint
        || reference["modelSha256"] != map.model_fingerprint
        || reference["modelFingerprintMatchesMap"] != true
    {
        return Err("reference/prior report fingerprint mismatch".into());
    }
    let expected_id = reference["expectedFragment"]
        .as_str()
        .ok_or("reference has no expected fragment")?;
    let requested = map
        .triangles
        .iter()
        .find(|t| t.id.stable_key() == expected_id)
        .ok_or("requested fragment absent")?;
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let point = requested
        .paint_uv
        .map(Vec2::from_array)
        .into_iter()
        .sum::<Vec2>()
        / 3.;
    let (index, bary) = topology
        .locate(&requested.part_id, point)
        .ok_or("public locate missing")?;
    let actual = &map.triangles[index];
    let (origin, u, v) = topology
        .tangent_frame(&requested.part_id, point)
        .ok_or("public frame refused")?;
    let same_source = reference["sourceMesh"] == actual.id.source_mesh
        && reference["primitive"] == actual.id.primitive
        && reference["sourceTriangle"] == actual.id.source_triangle;
    let original: [[f64; 3]; 3] =
        serde_json::from_value(reference["originalWorldCornersF64"].clone())?;
    let original = original.map(DVec3::from_array);
    let cross = (original[1] - original[0]).cross(original[2] - original[0]);
    if !cross.is_finite() || cross.length_squared() == 0. {
        return Err("independent original source normal is invalid".into());
    }
    let original_normal = cross.normalize();
    let u = u.as_dvec3() * 1000.;
    let v = v.as_dvec3() * 1000.;
    let normal = u.cross(v).normalize();
    let unit_error = (u.length() - 1.).abs().max((v.length() - 1.).abs());
    let orthogonal = u.dot(v).abs();
    let normal_dot = normal.dot(original_normal);
    let normal_error = 1. - normal_dot.abs().clamp(0., 1.);
    let raw = actual.world.map(|p| DVec3::from_array(p.map(f64::from)));
    let normalized_bary = actual.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        b / b.element_sum()
    });
    let original_mapped =
        normalized_bary.map(|b| original[0] * b.x + original[1] * b.y + original[2] * b.z);
    let max_corner_residual = original_mapped
        .into_iter()
        .zip(raw)
        .map(|(a, b)| a.distance(b) * 1000.)
        .fold(0., f64::max);
    let expected_origin =
        raw[0] * f64::from(bary.x) + raw[1] * f64::from(bary.y) + raw[2] * f64::from(bary.z);
    let world_error = origin.as_dvec3().distance(expected_origin) * 1000.;
    let finite = origin.is_finite() && u.is_finite() && v.is_finite() && normal.is_finite();
    // These limits are exactly the existing all-public-frame audit limits.
    let normal_verified = same_source && finite && max_corner_residual <= 0.005;
    let passes = normal_verified
        && unit_error <= 0.01
        && orthogonal <= 0.001
        && normal_error <= 1e-5
        && world_error <= 0.2;
    let uncovered = prior["unverifiedSourcePlaneFrames"]
        .as_array()
        .ok_or("prior report missing unverified rows")?;
    let matching_prior = uncovered
        .iter()
        .filter(|r| r["requested"] == expected_id && r["located"] == actual.id.stable_key())
        .count();
    let prior_counts = &prior["summary"]["counts"];
    let prior_verified = prior_counts["framesWithIndependentlyVerifiedStableSourcePlane"]
        .as_u64()
        .ok_or("prior count missing")?;
    let requested_count = prior_counts["requestedFrames"]
        .as_u64()
        .ok_or("prior requested count missing")?;
    let all_unverified_closed = matching_prior == 1 && uncovered.len() == 1 && passes;
    let report = json!({
        "schema":"itasha-original-glb-public-source-frame-supplement-v1",
        "mapPath":args[0],"paintFingerprint":map.paint_fingerprint,"modelFingerprint":map.model_fingerprint,
        "originalReferencePath":args[1],"priorAllPublicFrameReport":args[2],
        "definition":"Original GLB POSITION plus full node hierarchy independently supplies world vertices and source normal. The actual public f32 UV centroid is submitted to public locate and tangent_frame. Compare the returned frame with the actual located source face; do not substitute a stable retained template. Preserve the existing audit thresholds: raw original-source corner residual<=.005mm, unit error<=.01, abs axis dot<=.001, normal 1-absdot<=1e-5, origin-vs-public-located-raw-world error<=.2mm. This supplement closes only matching prior unverified rows and leaves the prior report intact.",
        "originalSourceReference":reference,
        "publicFrame":{
            "requested":requested.id.stable_key(),"located":actual.id.stable_key(),"partId":requested.part_id,"chartId":requested.chart_id,
            "publicPaintPointF32":point.to_array(),"publicLocatedBarycentricF32":bary.to_array(),
            "publicOriginWorldF32":origin.to_array(),"publicUWorldPerMm":(u/1000.).to_array(),"publicVWorldPerMm":(v/1000.).to_array(),
            "publicUnitNormalF64":normal.to_array(),"independentOriginalUnitNormalF64":original_normal.to_array(),
            "actualLocatedSameOriginalSourceAsReference":same_source,"maxRawCornerResidualMm":max_corner_residual,
            "axisLengthsPerMm":[u.length(),v.length()],"axisUnitError":unit_error,"orthogonalDot":orthogonal,
            "orientedOriginalNormalDot":normal_dot,"independentOriginalNormalErrorOneMinusAbsDot":normal_error,
            "publicOriginVsLocatedRawWorldErrorMm":world_error,"finite":finite,"independentOriginalNormalVerified":normal_verified,"passes":passes
        },
        "combinedSummary":{
            "requestedFrames":requested_count,"priorStableTemplateVerifiedFrames":prior_verified,
            "supplementOriginalGlbVerifiedFrames":if all_unverified_closed {1}else{0},
            "framesWithIndependentlyVerifiedSourcePlane":prior_verified + u64::from(all_unverified_closed),
            "remainingUnverifiedSourcePlaneFrames":uncovered.len() - usize::from(all_unverified_closed),
            "priorFailedFrames":prior["summary"]["failedFrames"],"supplementFailedFrames":usize::from(!passes),
            "allRequestedFramesSourceNormalVerified":all_unverified_closed && prior_verified + 1 == requested_count,
            "maxNormalOneMinusAbsDot":prior["summary"]["maxNormalOneMinusAbsDot"].as_f64().unwrap_or(0.).max(normal_error)
        }
    });
    std::fs::write(&args[3], serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report["publicFrame"])?);
    println!(
        "{}",
        serde_json::to_string_pretty(&report["combinedSummary"])?
    );
    if !passes || !all_unverified_closed {
        return Err(
            "supplement did not close the outstanding original source normal verification".into(),
        );
    }
    Ok(())
}
