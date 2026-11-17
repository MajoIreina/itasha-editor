//! Every requested frame is obtained through public locate and tangent_frame.
use glam::{DMat3, DVec3, Vec2};
use itasha_surface_map::{PaintSurfaceMap, PaintTriangle, topology::Topology};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Instant;

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn source(t: &PaintTriangle) -> [DVec3; 3] {
    t.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        b / b.element_sum()
    })
}
fn source_det(t: &PaintTriangle) -> f64 {
    let [a, b, c] = t.source_barycentric.map(|b| b.map(f64::from));
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0]))
        / (a.into_iter().sum::<f64>() * b.into_iter().sum::<f64>() * c.into_iter().sum::<f64>())
}
fn height(w: [DVec3; 3]) -> f64 {
    let longest = (0..3)
        .map(|i| w[i].distance(w[(i + 1) % 3]))
        .fold(0., f64::max);
    (w[1] - w[0]).cross(w[2] - w[0]).length() / longest
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    map.validate()?;
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let mut templates: HashMap<(&str, u32, u32), (f64, usize, DMat3, DVec3)> = HashMap::new();
    for (i, t) in map.triangles.iter().enumerate() {
        let det = source_det(t).abs();
        if det <= 1e-12 || height(world(t)) <= 2e-6 {
            continue;
        }
        let key = (
            t.id.source_mesh.as_str(),
            t.id.primitive,
            t.id.source_triangle,
        );
        if templates.get(&key).is_some_and(|p| p.0 >= det) {
            continue;
        }
        let b = source(t);
        let w = world(t);
        let a = DMat3::from_cols(w[0], w[1], w[2]) * DMat3::from_cols(b[0], b[1], b[2]).inverse();
        let n = (a.y_axis - a.x_axis).cross(a.z_axis - a.x_axis).normalize();
        templates.insert(key, (det, i, a, n));
    }
    let start = Instant::now();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut micro_charts = BTreeSet::new();
    let mut failures = Vec::new();
    let mut identity = Vec::new();
    let mut unverified = Vec::new();
    let mut max_unit_error = 0_f64;
    let mut max_orthogonal = 0_f64;
    let mut max_normal_error = 0_f64;
    let mut max_world_error = 0_f64;
    let mut worst = Vec::new();
    for (i, t) in map.triangles.iter().enumerate() {
        *counts.entry("requestedFrames").or_default() += 1;
        let micro = t.chart_id.contains("micro");
        let thin = height(world(t)) * 1000. < 0.002;
        if micro {
            micro_charts.insert(t.chart_id.as_str());
            *counts.entry("requestedMicroChartFragments").or_default() += 1;
        }
        if thin {
            *counts
                .entry("requestedRawWorldAltitudeBelow2Micrometres")
                .or_default() += 1;
        }
        let at = t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.;
        let located = topology.locate(&t.part_id, at);
        let frame = topology.tangent_frame(&t.part_id, at);
        let Some((j, bary)) = located else {
            *counts.entry("locateMissing").or_default() += 1;
            failures.push(json!({"requested":t.id.stable_key(),"failure":"locate-missing","sourceCenter":at.to_array(),"microChart":micro,"thin":thin}));
            continue;
        };
        let actual = &map.triangles[j];
        if i != j {
            identity.push(json!({"requested":t.id.stable_key(),"located":actual.id.stable_key(),"requestedChart":t.chart_id,"locatedChart":actual.chart_id,"sameOriginalSource":t.id.source_mesh==actual.id.source_mesh&&t.id.primitive==actual.id.primitive&&t.id.source_triangle==actual.id.source_triangle,"microChart":micro,"thin":thin}));
        }
        let Some((origin, u, v)) = frame else {
            *counts.entry("frameRefused").or_default() += 1;
            failures.push(json!({"requested":t.id.stable_key(),"located":actual.id.stable_key(),"failure":"public-frame-refused","microChart":micro,"thin":thin}));
            continue;
        };
        *counts.entry("finiteFramesReturned").or_default() += 1;
        let u = u.as_dvec3() * 1000.;
        let v = v.as_dvec3() * 1000.;
        let lengths = [u.length(), v.length()];
        let unit_error = lengths
            .map(|v| (v - 1.).abs())
            .into_iter()
            .fold(0., f64::max);
        let orthogonal = u.dot(v).abs();
        max_unit_error = max_unit_error.max(unit_error);
        max_orthogonal = max_orthogonal.max(orthogonal);
        let key = (
            actual.id.source_mesh.as_str(),
            actual.id.primitive,
            actual.id.source_triangle,
        );
        let template = templates.get(&key);
        let normal_comparison = template.map(|(_, reference, a, n)| {
            let residual = source(actual)
                .map(|b| *a * b)
                .into_iter()
                .zip(world(actual))
                .map(|(a, b)| a.distance(b))
                .fold(0., f64::max)
                * 1000.;
            let dot = u.cross(v).normalize().dot(*n).abs().clamp(0., 1.);
            (1. - dot, residual, *reference)
        });
        let verified = normal_comparison.is_some_and(|(_, residual, _)| residual <= 0.005);
        let normal_error = normal_comparison.filter(|_| verified).map(|x| x.0);
        if let Some(error) = normal_error {
            max_normal_error = max_normal_error.max(error);
            *counts
                .entry("framesWithIndependentlyVerifiedStableSourcePlane")
                .or_default() += 1;
        } else {
            unverified.push(json!({"requested":t.id.stable_key(),"located":actual.id.stable_key(),"stableSourceAvailable":template.is_some(),"maxRawCornerResidualMm":normal_comparison.map(|x|x.1),"microChart":micro,"thin":thin}));
        }
        let w = world(actual);
        let expected =
            w[0] * f64::from(bary.x) + w[1] * f64::from(bary.y) + w[2] * f64::from(bary.z);
        let world_error = origin.as_dvec3().distance(expected) * 1000.;
        max_world_error = max_world_error.max(world_error);
        let finite = origin.is_finite() && u.is_finite() && v.is_finite();
        let passes = finite
            && unit_error <= 0.01
            && orthogonal <= 0.001
            && normal_error.is_none_or(|a| a <= 1e-5)
            && world_error <= 0.2;
        let row = json!({"requested":t.id.stable_key(),"located":actual.id.stable_key(),"partId":t.part_id,"chartId":t.chart_id,"microChart":micro,"thin":thin,"axisLengthsPerMm":lengths,"orthogonalDot":orthogonal,"independentOriginalNormalErrorOneMinusAbsDot":normal_error,"independentStableReference":normal_comparison.map(|x|map.triangles[x.2].id.stable_key()),"maxStableSourceCornerResidualMm":normal_comparison.map(|x|x.1),"publicOriginVsLocatedRawWorldErrorMm":world_error,"finite":finite,"passes":passes});
        if !passes {
            failures.push(row.clone());
        }
        worst.push((normal_error.unwrap_or(-1.), row));
        if (i + 1) % 10000 == 0 {
            eprintln!(
                "checked {} public frames in {:.1}s",
                i + 1,
                start.elapsed().as_secs_f64()
            );
        }
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    worst.truncate(32);
    let summary = json!({"counts":counts,"uniqueRequestedMicroCharts":micro_charts.len(),"identityMismatches":identity.len(),"failedFrames":failures.len(),"unverifiedSourcePlaneFrames":unverified.len(),"maxAxisUnitError":max_unit_error,"maxOrthogonalDot":max_orthogonal,"maxNormalOneMinusAbsDot":max_normal_error,"maxOriginVsLocatedRawWorldErrorMm":max_world_error,"elapsedSeconds":start.elapsed().as_secs_f64()});
    let report = json!({"schema":"itasha-all-public-physical-frames-audit-v1","map":args[0],"paintFingerprint":map.paint_fingerprint,"definition":"All retained triangle f32 UV centroids are actually submitted to public locate and tangent_frame. Identity differences remain explicit; the returned frame is evaluated against its actually located original source face. An independent raw-world/source-barycentric affine reconstruction from the largest homogeneous-source-domain stable retained fragment (minimum raw altitude >2um, source determinant >1e-12) supplies the plane normal only when every located raw corner is within5um. Axes are tested in physical world millimetres, with unit error<=1%, abs dot<=.001, normal 1-absdot<=1e-5 and origin error<=.2mm. Missing, refused and unverified cases are reported separately. No production private frames/coordinates are used.","summary":summary,"identityMismatches":identity,"failures":failures,"unverifiedSourcePlaneFrames":unverified,"worstVerifiedNormalFrames":worst.into_iter().map(|x|x.1).collect::<Vec<_>>()});
    std::fs::write(&args[1], serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report["summary"])?);
    Ok(())
}
