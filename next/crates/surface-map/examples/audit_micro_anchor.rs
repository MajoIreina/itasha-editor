//! Read-only runtime placement probe for source-positive, numerically thin fragments.
use glam::{DMat2, DMat3, DVec2, DVec3, Vec2};
use itasha_surface_map::{CoverageFragment, PaintSurfaceMap, PaintTriangle, topology::Topology};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn area(w: [DVec3; 3]) -> f64 {
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6
}
fn angle(a: DVec3, b: DVec3) -> f64 {
    a.normalize()
        .dot(b.normalize())
        .clamp(-1., 1.)
        .acos()
        .to_degrees()
}
fn bary(t: &PaintTriangle) -> [DVec3; 3] {
    t.source_barycentric.map(|p| {
        let v = DVec3::from_array(p.map(f64::from));
        v / v.element_sum()
    })
}
fn axes(w: [DVec3; 3], q: [DVec2; 3]) -> Option<[DVec3; 2]> {
    let a = (w[1] - w[0]) * 1000.;
    let b = (w[2] - w[0]) * 1000.;
    let x = a.try_normalize()?;
    let y = (b - x * b.dot(x)).try_normalize()?;
    let physical = DMat2::from_cols(DVec2::new(a.length(), 0.), DVec2::new(b.dot(x), b.dot(y)));
    let j = DMat2::from_cols(q[1] - q[0], q[2] - q[0]) * physical.inverse();
    // Independent positive square-root polar factor of J^T J.
    let c = j.transpose() * j;
    let s = c.determinant().max(0.).sqrt();
    let root = (c + DMat2::IDENTITY * s) / (c.x_axis.x + c.y_axis.y + 2. * s).sqrt();
    let r = j * root.inverse();
    let result = [
        x * r.x_axis.x + y * r.y_axis.x,
        x * r.x_axis.y + y * r.y_axis.y,
    ];
    result.iter().all(|a| a.is_finite()).then_some(result)
}
fn fragment_axes(f: &CoverageFragment) -> Option<[DVec3; 2]> {
    let w: [DVec3; 3] = std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
    if area(w) < 0.1 {
        return None;
    }
    let q: [DVec2; 3] = std::array::from_fn(|i| Vec2::from_array(f.image_uv[i]).as_dvec2() * 300.);
    let inverse = DMat2::from_cols(q[1] - q[0], q[2] - q[0]).inverse();
    let apply = |v: DVec2| (w[1] - w[0]) * v.x + (w[2] - w[0]) * v.y;
    let a = [apply(inverse.x_axis), apply(inverse.y_axis)];
    a.iter().all(|v| v.is_finite()).then_some(a)
}
fn coverage_summary(coverage: &[CoverageFragment]) -> Value {
    let mut by_part: BTreeMap<&str, f64> = BTreeMap::new();
    let mut rows = Vec::new();
    for f in coverage {
        let w = std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
        let a = area(w);
        *by_part.entry(&f.part_id).or_default() += a;
        rows.push(json!({"triangle":f.triangle.stable_key(),"areaMm2":a}));
    }
    rows.sort_by(|a, b| {
        b["areaMm2"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["areaMm2"].as_f64().unwrap())
    });
    rows.truncate(5);
    json!({"fragments":coverage.len(),"totalAreaMm2":by_part.values().sum::<f64>(),"areaByPartMm2":by_part,"largestFragments":rows})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let map_path = PathBuf::from(args.first().ok_or("map path required")?);
    let output = PathBuf::from(args.get(1).ok_or("output path required")?);
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&map_path)?)?;
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let wanted: Vec<u64> = if args.len() > 2 {
        args[2..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?
    } else {
        vec![
            18446640630114027776,
            18446640630114024220,
            18446640630114024529,
            18446640630114026217,
        ]
    };
    let mut rows = Vec::new();
    for fragment in wanted {
        let t = map
            .triangles
            .iter()
            .find(|t| t.id.fragment == fragment)
            .ok_or("fragment missing")?;
        let stable = map
            .triangles
            .iter()
            .filter(|q| {
                q.id.source_mesh == t.id.source_mesh
                    && q.id.source_triangle == t.id.source_triangle
                    && q.part_id == t.part_id
            })
            .max_by(|a, b| area(world(a)).total_cmp(&area(world(b))))
            .ok_or("source siblings missing")?;
        let center = t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.;
        let stable_center = stable
            .paint_uv
            .map(Vec2::from_array)
            .into_iter()
            .sum::<Vec2>()
            / 3.;
        let located = topology
            .locate(&t.part_id, center)
            .map(|(i, b)| (topology.triangles[i].id.stable_key(), b.to_array()));
        let runtime = topology.tangent_frame(&t.part_id, center);
        let stable_runtime = topology.tangent_frame(&stable.part_id, stable_center);
        let runtime_frame = runtime.map(|(o,u,v)|json!({"origin":o.to_array(),"imageXPerMm":u.to_array(),"imageYPerMm":v.to_array()}));
        let stable_runtime_frame = stable_runtime.map(|(o,u,v)|json!({"origin":o.to_array(),"imageXPerMm":u.to_array(),"imageYPerMm":v.to_array()}));
        let runtime_axes = runtime.map(|(_, u, v)| [u.as_dvec3(), v.as_dvec3()]);
        let stable_runtime_axes = stable_runtime.map(|(_, u, v)| [u.as_dvec3(), v.as_dvec3()]);
        let coverage = topology.coverage_at(&t.part_id, center.to_array(), [300., 300.], 0., true);
        let reference = topology.coverage_at(
            &stable.part_id,
            stable_center.to_array(),
            [300., 300.],
            0.,
            true,
        );
        let ref_axes: BTreeMap<_, _> = reference
            .iter()
            .filter_map(|f| fragment_axes(f).map(|a| (f.triangle.stable_key(), a)))
            .collect();
        let mut comparisons = Vec::new();
        for f in &coverage {
            if let (Some(a), Some(b)) = (fragment_axes(f), ref_axes.get(&f.triangle.stable_key())) {
                comparisons.push(json!({"triangle":f.triangle.stable_key(),"areaMm2":area(std::array::from_fn(|i|DVec3::from_array(f.world[i].map(f64::from)))),"xAngleDeg":angle(a[0],b[0]),"yAngleDeg":angle(a[1],b[1])}));
            }
        }
        comparisons.sort_by(|a, b| {
            b["areaMm2"]
                .as_f64()
                .unwrap()
                .total_cmp(&a["areaMm2"].as_f64().unwrap())
        });
        let common_area: f64 = comparisons
            .iter()
            .map(|r| r["areaMm2"].as_f64().unwrap())
            .sum();
        let max_x = comparisons
            .iter()
            .map(|r| r["xAngleDeg"].as_f64().unwrap())
            .fold(0., f64::max);
        let max_y = comparisons
            .iter()
            .map(|r| r["yAngleDeg"].as_f64().unwrap())
            .fold(0., f64::max);
        let source_vertices =
            DMat3::from_cols(world(stable)[0], world(stable)[1], world(stable)[2])
                * DMat3::from_cols(bary(stable)[0], bary(stable)[1], bary(stable)[2]).inverse();
        let reconstructed = bary(t).map(|b| source_vertices * b);
        let q = t.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
        let stable_q = stable.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
        let chart_vertices =
            DMat3::from_cols(
                stable_q[0].extend(0.),
                stable_q[1].extend(0.),
                stable_q[2].extend(0.),
            ) * DMat3::from_cols(bary(stable)[0], bary(stable)[1], bary(stable)[2]).inverse();
        let reconstructed_chart = bary(t).map(|b| (chart_vertices * b).truncate());
        let reconstructed_axes = axes(reconstructed, q);
        let full_affine_axes = axes(reconstructed, reconstructed_chart);
        let stored_axes = axes(world(t), q);
        let stable_axes = axes(world(stable), stable_q);
        let compare_axes = |a: Option<[DVec3; 2]>, b: Option<[DVec3; 2]>| {
            a.zip(b)
                .map(|(a, b)| [angle(a[0], b[0]), angle(a[1], b[1])])
        };
        let normal = |w: [DVec3; 3]| (w[1] - w[0]).cross(w[2] - w[0]).normalize();
        let axis_error = stored_axes
            .zip(reconstructed_axes)
            .map(|(a, b)| [angle(a[0], b[0]), angle(a[1], b[1])]);
        let same_source_comparisons: Vec<_> = comparisons
            .iter()
            .filter(|r| {
                r["triangle"].as_str().unwrap().starts_with(&format!(
                    "{}:{}:{}:",
                    t.id.source_mesh, t.id.primitive, t.id.source_triangle
                ))
            })
            .cloned()
            .collect();
        rows.push(json!({"triangle":t.id.stable_key(),"chart":t.chart_id,"sourceCenter":center.to_array(),"located":located,"runtimeFrame":runtime_frame,"stableRuntimeFrame":stable_runtime_frame,
            "stableSibling":stable.id.stable_key(),"stableSiblingChart":stable.chart_id,"stableSiblingAreaMm2":area(world(stable)),
            "storedAreaMm2":area(world(t)),"reconstructedAreaMm2":area(reconstructed),
            "storedVsReconstructedNormalDeg":angle(normal(world(t)),normal(reconstructed)),"storedVsReconstructedAxesDeg":axis_error,
            "storedVsStableSiblingAxesDeg":compare_axes(stored_axes,stable_axes),"worldOnlyReconstructedVsStableSiblingAxesDeg":compare_axes(reconstructed_axes,stable_axes),"fullAffineReconstructedVsStableSiblingAxesDeg":compare_axes(full_affine_axes,stable_axes),
            "runtimeVsStableRuntimeAxesDeg":compare_axes(runtime_axes,stable_runtime_axes),"runtimeVsFullAffineAxesDeg":compare_axes(runtime_axes,full_affine_axes),
            "sourceAffineCornerDisplacementMm":std::array::from_fn::<_,3,_>(|i|reconstructed[i].distance(world(t)[i])*1000.),
            "reconstructedNormal":normal(reconstructed).to_array(),"storedNormal":normal(world(t)).to_array(),
            "microSeedCoverage":coverage_summary(&coverage),"stableSeedCoverage":coverage_summary(&reference),
            "commonMeasuredAreaMm2":common_area,"commonMaxImageXAxisDifferenceDeg":max_x,"commonMaxImageYAxisDifferenceDeg":max_y,
            "sameSourceFaceOutputAxisComparisons":same_source_comparisons,
            "largestCommonOutputAxisComparisons":comparisons.into_iter().take(10).collect::<Vec<_>>() }));
    }
    let report = json!({"map":map_path,"paintFingerprint":map.paint_fingerprint,"sizeMm":[300,300],"rotationRad":0,"definition":"Runtime public locate/tangent_frame/coverage_at at f32 chart centroids. Independent f64 homogeneous source-face reconstruction from the largest sibling; common large output fragments compare generated world image axes against placement from the stable sibling centroid.","results":rows});
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("saved {}", output.display());
    Ok(())
}
