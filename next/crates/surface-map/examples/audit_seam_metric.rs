//! Independent physical-millimetre audit of the committed vehicle seams.
//! Run with --mode full for every authored seam, three interior edge samples,
//! and both directions. Additional aspect/rotation cases use spatial strata.
use glam::{DMat2, DQuat, DVec2, DVec3, Vec2};
use itasha_surface_map::{
    CoverageFragment, EdgeConnection, PaintSurfaceMap, PaintTriangle, TriangleId, barycentric_2d,
    topology::Topology,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    time::Instant,
};

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn centroid(t: &PaintTriangle) -> DVec3 {
    world(t).into_iter().sum::<DVec3>() / 3.
}
fn homogeneous_source_det(t: &PaintTriangle) -> f64 {
    let [a, b, c] = t.source_barycentric.map(|b| b.map(f64::from));
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0]))
        / (a.into_iter().sum::<f64>() * b.into_iter().sum::<f64>() * c.into_iter().sum::<f64>())
}
fn physical(w: [DVec3; 3]) -> Option<DMat2> {
    let a = (w[1] - w[0]) * 1000.;
    let b = (w[2] - w[0]) * 1000.;
    let length = a.length();
    let height = a.cross(b).length() / length;
    (length > 1e-10 && height > 1e-10).then(|| {
        DMat2::from_cols(
            DVec2::new(length, 0.),
            DVec2::new(a.dot(b) / length, height),
        )
    })
}
fn singular(j: DMat2) -> [f64; 2] {
    let trace = j.x_axis.length_squared() + j.y_axis.length_squared();
    let det = j.determinant().abs();
    let largest = ((trace + (trace * trace - 4. * det * det).max(0.).sqrt()) * 0.5).sqrt();
    [det / largest, largest]
}
fn chart_metric(t: &PaintTriangle) -> Option<([f64; 2], f64)> {
    let p = physical(world(t))?;
    let uv = t.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
    Some((
        singular(DMat2::from_cols(uv[1] - uv[0], uv[2] - uv[0]) * p.inverse()),
        p.determinant().abs() * 0.5,
    ))
}
fn weighted(values: &[(f64, f64)], quantile: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let threshold = sorted.iter().map(|x| x.1).sum::<f64>() * quantile;
    let mut sum = 0.;
    for &(value, weight) in &sorted {
        sum += weight;
        if sum >= threshold {
            return value;
        }
    }
    sorted.last().map_or(0., |x| x.0)
}
fn chart_report(map: &PaintSurfaceMap) -> Vec<Value> {
    let mut parts: BTreeMap<&str, Vec<([f64; 2], f64)>> = BTreeMap::new();
    for t in &map.triangles {
        if let Some(metric) = chart_metric(t) {
            parts.entry(&t.part_id).or_default().push(metric);
        }
    }
    parts.into_iter().map(|(part, rows)| {
        let values = |index: usize| rows.iter().map(|(s, area)| (s[index], *area)).collect::<Vec<_>>();
        let condition = rows.iter().map(|(s, area)| (s[1] / s[0], *area)).collect::<Vec<_>>();
        json!({"partId": part, "triangles":rows.len(), "areaMm2":rows.iter().map(|x|x.1).sum::<f64>(),
            "chartUnitsPerPhysicalMmMinP05P50P95":[weighted(&values(0),0.05),weighted(&values(0),0.5),weighted(&values(0),0.95)],
            "chartUnitsPerPhysicalMmMaxP05P50P95":[weighted(&values(1),0.05),weighted(&values(1),0.5),weighted(&values(1),0.95)],
            "chartConditionP50P95Max":[weighted(&condition,0.5),weighted(&condition,0.95),condition.iter().map(|x|x.0).fold(0.,f64::max)]})
    }).collect()
}
fn area(f: &CoverageFragment) -> f64 {
    let w = f
        .world
        .iter()
        .map(|p| DVec3::from_array(p.map(f64::from)))
        .collect::<Vec<_>>();
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5
}
// Measure the generated image coordinates, independently of source paint UVs.
fn fragment_metric(f: &CoverageFragment, size: [f32; 2]) -> Option<([f64; 2], [DVec3; 2], DVec3)> {
    let w: [DVec3; 3] = std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
    let p = physical(w)?;
    // Very thin clipped fragments amplify f32 output rounding. They remain in
    // area/coverage checks, but are excluded from the numerical derivative.
    let a = p.x_axis.length();
    let b = p.y_axis.length();
    if p.determinant().abs() < 0.01 || p.determinant().abs() / (a * b) < 0.002 {
        return None;
    }
    let uv: [DVec2; 3] = std::array::from_fn(|i| {
        Vec2::from_array(f.image_uv[i]).as_dvec2() * Vec2::from_array(size).as_dvec2()
    });
    let image = DMat2::from_cols(uv[1] - uv[0], uv[2] - uv[0]);
    if image.determinant().abs() < 1e-10 {
        return None;
    }
    let inverse = image.inverse();
    let axis = |v: DVec2| ((w[1] - w[0]) * v.x + (w[2] - w[0]) * v.y) * 1000.;
    Some((
        singular(image * p.inverse()),
        [axis(inverse.x_axis), axis(inverse.y_axis)],
        (w[1] - w[0]).cross(w[2] - w[0]).normalize(),
    ))
}
fn direction_witness(
    fragments: &[CoverageFragment],
    source: &PaintTriangle,
    target: &PaintTriangle,
    size: [f32; 2],
) -> Value {
    let get = |id: &TriangleId| {
        fragments
            .iter()
            .filter(|f| &f.triangle == id)
            .filter_map(|f| fragment_metric(f, size).map(|m| (area(f), m)))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|x| x.1)
    };
    let (Some((_, a, mut an)), Some((_, b, mut bn))) = (get(&source.id), get(&target.id)) else {
        return json!({"usable":false});
    };
    let origin = (centroid(source) + centroid(target)) * 0.5;
    let outward = origin - DVec3::new(0., 0.7, 0.);
    if an.dot(outward) < 0. {
        an = -an;
    }
    if bn.dot(outward) < 0. {
        bn = -bn;
    }
    let turn = if an.dot(bn) < -0.99999 {
        DQuat::from_axis_angle(a[1].normalize(), std::f64::consts::PI)
    } else {
        DQuat::from_rotation_arc(an, bn)
    };
    let degrees = |x: DVec3, y: DVec3| {
        x.normalize()
            .dot(y.normalize())
            .clamp(-1., 1.)
            .acos()
            .to_degrees()
    };
    json!({"usable":true,"normalTurnDeg":degrees(an,bn),"imageXAxisTurnResidualDeg":degrees(turn*a[0],b[0]),"imageYAxisTurnResidualDeg":degrees(turn*a[1],b[1]),"sameExteriorHandedness":a[0].cross(a[1]).dot(an).signum()==b[0].cross(b[1]).dot(bn).signum()})
}
// A common exterior view is independent of chart packing and vertex winding.
// Keep exact seam-triangle witnesses above too: rolled edges can face away
// from the visible outer skin and must not silently disappear from the report.
fn exterior_witness(
    fragments: &[CoverageFragment],
    source: &PaintTriangle,
    target: &PaintTriangle,
    size: [f32; 2],
) -> Value {
    let origin = (centroid(source) + centroid(target)) * 0.5;
    let outward = (origin - DVec3::new(0., 0.7, 0.)).normalize();
    let side = |part: &str| {
        let mut signs = [0.; 2];
        let mut axes = [DVec3::ZERO; 2];
        let mut normal = DVec3::ZERO;
        let mut measured = 0.;
        for f in fragments.iter().filter(|f| f.part_id == part) {
            let w: [DVec3; 3] =
                std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
            let n = (w[1] - w[0]).cross(w[2] - w[0]);
            let facing = n.dot(outward);
            if n.length() < 1e-14
                || facing.abs() < n.length() * 0.35
                || ((w[0] + w[1] + w[2]) / 3.).distance(origin) > 0.4
            {
                continue;
            }
            let uv: [DVec2; 3] =
                std::array::from_fn(|i| Vec2::from_array(f.image_uv[i]).as_dvec2());
            let determinant = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]);
            if determinant.abs() < 1e-16 {
                continue;
            }
            let weight = facing.abs() * 0.5;
            signs[usize::from(determinant * facing > 0.)] += weight;
            if let Some((_, a, raw)) = fragment_metric(f, size) {
                measured += weight;
                axes[0] += a[0] * weight;
                axes[1] += a[1] * weight;
                normal += raw * weight * if facing < 0. { -1. } else { 1. };
            }
        }
        (signs, axes, normal, measured)
    };
    let (a, aa, an, am) = side(&source.part_id);
    let (b, ba, bn, bm) = side(&target.part_id);
    let usable = a.iter().sum::<f64>() > 1e-7 && b.iter().sum::<f64>() > 1e-7;
    let mut value = json!({"usable":usable,"sourceProjectedAreaMm2":a.map(|v|v*1e6),"targetProjectedAreaMm2":b.map(|v|v*1e6),"sameDominantHandedness":if usable{Some((a[1]>a[0])==(b[1]>b[0]))}else{None},"sourceFoldFraction":a[0].min(a[1])/a.iter().sum::<f64>().max(1e-30),"targetFoldFraction":b[0].min(b[1])/b.iter().sum::<f64>().max(1e-30)});
    if am > 1e-7 && bm > 1e-7 && an.length() > 1e-14 && bn.length() > 1e-14 {
        let n = an.normalize();
        let tn = bn.normalize();
        if n.dot(tn) > -0.99999 {
            let turn = DQuat::from_rotation_arc(n, tn);
            let angle = |a: DVec3, b: DVec3| {
                a.normalize()
                    .dot(b.normalize())
                    .clamp(-1., 1.)
                    .acos()
                    .to_degrees()
            };
            value["averagedXAxisTurnResidualDeg"] = json!(angle(turn * aa[0], ba[0]));
            value["averagedYAxisTurnResidualDeg"] = json!(angle(turn * aa[1], ba[1]));
        }
    }
    value
}

fn t_junction_report(
    edges: &HashMap<(String, [i64; 3], [i64; 3]), Vec<usize>>,
    parent: &[usize],
    component_areas: &HashMap<usize, f64>,
    map: &PaintSurfaceMap,
) -> Value {
    type Position = [i64; 3];
    let mut vertices: HashMap<(&str, Position), BTreeSet<usize>> = HashMap::new();
    for ((part, a, b), indices) in edges.iter().filter(|(_, v)| v.len() == 1) {
        for point in [a, b] {
            vertices
                .entry((part.as_str(), *point))
                .or_default()
                .insert(indices[0]);
        }
    }
    let cell = 5000_i64;
    let grid_key = |p: Position| p.map(|v| v.div_euclid(cell));
    let mut grid: HashMap<(&str, Position), Vec<Position>> = HashMap::new();
    for &(part, p) in vertices.keys() {
        grid.entry((part, grid_key(p))).or_default().push(p);
    }
    let mut by_part: BTreeMap<&str, Value> = BTreeMap::new();
    let mut examples = Vec::new();
    let mut boundary_edges = 0;
    let mut junction_edges = 0;
    let mut intercomponent_edges = 0;
    let mut large_intercomponent_edges = 0;
    let mut disconnected_pairs = BTreeSet::new();
    let mut large_disconnected_pairs = BTreeSet::new();
    for ((part, a, b), indices) in edges.iter().filter(|(_, v)| v.len() == 1) {
        boundary_edges += 1;
        let entry = by_part.entry(part).or_insert_with(
            || json!({"unpairedEdges":0,"tJunctionEdges":0,"interComponentTJunctionEdges":0}),
        );
        entry["unpairedEdges"] = json!(entry["unpairedEdges"].as_u64().unwrap() + 1);
        let start = DVec3::from_array(a.map(|v| v as f64));
        let end = DVec3::from_array(b.map(|v| v as f64));
        let delta = end - start;
        let length = delta.length();
        if length < 4. {
            continue;
        }
        let steps = (length / cell as f64).ceil() as usize;
        let mut candidates = BTreeSet::new();
        for step in 0..=steps {
            let p = start + delta * (step as f64 / steps as f64);
            let center = grid_key(p.to_array().map(|v| v.round() as i64));
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        if let Some(points) = grid
                            .get(&(part.as_str(), [center[0] + x, center[1] + y, center[2] + z]))
                        {
                            candidates.extend(points.iter().copied());
                        }
                    }
                }
            }
        }
        let mut hits = Vec::new();
        let mut separated = false;
        let mut large_separated = false;
        for point in candidates {
            if point == *a || point == *b {
                continue;
            }
            let p = DVec3::from_array(point.map(|v| v as f64));
            let t = (p - start).dot(delta) / delta.length_squared();
            if !(1e-5..0.99999).contains(&t) || p.distance(start + delta * t) > 1.5 {
                continue;
            }
            let incident = &vertices[&(part.as_str(), point)];
            if !incident.iter().any(|&i| i != indices[0]) {
                continue;
            }
            let cross_component = incident.iter().any(|&i| parent[i] != parent[indices[0]]);
            separated |= cross_component;
            let source_root = parent[indices[0]];
            let mut target_components = BTreeSet::new();
            for &i in incident {
                let target_root = parent[i];
                target_components.insert(target_root);
                if source_root != target_root {
                    let pair = (source_root.min(target_root), source_root.max(target_root));
                    disconnected_pairs.insert(pair);
                    if component_areas[&source_root] > 0.01 && component_areas[&target_root] > 0.01
                    {
                        large_separated = true;
                        large_disconnected_pairs.insert(pair);
                    }
                }
            }
            hits.push(json!({"positionWorld":point.map(|v|v as f64/1e6),"edgeFraction":t,"distanceMicrometres":p.distance(start+delta*t),"separateComponent":cross_component,"incidentComponents":target_components.iter().map(|r|json!({"component":r,"areaMm2":component_areas[r]})).collect::<Vec<_>>(),"incidentTriangles":incident.iter().map(|&i|map.triangles[i].id.stable_key()).collect::<Vec<_>>()}));
        }
        if !hits.is_empty() {
            junction_edges += 1;
            entry["tJunctionEdges"] = json!(entry["tJunctionEdges"].as_u64().unwrap() + 1);
            if separated {
                intercomponent_edges += 1;
                entry["interComponentTJunctionEdges"] =
                    json!(entry["interComponentTJunctionEdges"].as_u64().unwrap() + 1);
            }
            if large_separated {
                large_intercomponent_edges += 1;
            }
            examples.push(json!({"partId":part,"triangle":map.triangles[indices[0]].id.stable_key(),"component":parent[indices[0]],"componentAreaMm2":component_areas[&parent[indices[0]]],"edgeLengthMm":length/1000.,"startWorld":a.map(|v|v as f64/1e6),"endWorld":b.map(|v|v as f64/1e6),"interComponent":separated,"bothComponentsAbove0_01Mm2":large_separated,"interiorBoundaryVertices":hits}));
        }
    }
    examples.sort_by(|a, b| {
        b["edgeLengthMm"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["edgeLengthMm"].as_f64().unwrap())
    });
    json!({"definition":"unpaired same-part geometric edge containing a boundary vertex strictly inside, distance <=1.5 micrometres after 1 micrometre coordinate quantization; structural candidates, not proof of visible missing coverage. Component identity is the independent current runtime source-aware graph.", "unpairedEdges":boundary_edges,"tJunctionEdges":junction_edges,"interComponentTJunctionEdges":intercomponent_edges,"interComponentPairs":disconnected_pairs.len(),"bothComponentsAbove0_01Mm2Edges":large_intercomponent_edges,"bothComponentsAbove0_01Mm2Pairs":large_disconnected_pairs.len(),"byPart":by_part,"longestExamples":examples})
}

fn anchor_audit(map: &PaintSurfaceMap, topology: &Topology) -> Value {
    type Key = (String, [i64; 3], [i64; 3]);
    type SourceKey = (String, String, u32, u32, [u64; 2], [u64; 2]);
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut parent = (0..map.triangles.len()).collect::<Vec<_>>();
    let mut geometric_parent = parent.clone();
    let mut edges: HashMap<Key, Vec<usize>> = HashMap::new();
    let mut source_edges: HashMap<SourceKey, Vec<usize>> = HashMap::new();
    let mut grid: HashMap<(&str, i32, i32), Vec<usize>> = HashMap::new();
    let cell = 64.;
    for (i, t) in map.triangles.iter().enumerate() {
        let source = t.source_barycentric.map(|b| {
            let b = DVec3::from_array(b.map(f64::from));
            (DVec2::new(b.y, b.z) / b.element_sum())
                .to_array()
                .map(f64::to_bits)
        });
        for e in 0..3 {
            let a = t.world[e].map(|v| (f64::from(v) * 1e6).round() as i64);
            let b = t.world[(e + 1) % 3].map(|v| (f64::from(v) * 1e6).round() as i64);
            edges
                .entry((t.part_id.clone(), a.min(b), a.max(b)))
                .or_default()
                .push(i);
            let a = source[e];
            let b = source[(e + 1) % 3];
            source_edges
                .entry((
                    t.part_id.clone(),
                    t.id.source_mesh.clone(),
                    t.id.primitive,
                    t.id.source_triangle,
                    a.min(b),
                    a.max(b),
                ))
                .or_default()
                .push(i);
        }
        let uv = t.paint_uv.map(Vec2::from_array);
        let min = uv.into_iter().fold(Vec2::splat(f32::INFINITY), Vec2::min);
        let max = uv
            .into_iter()
            .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
        for x in (min.x / cell).floor() as i32..=(max.x / cell).floor() as i32 {
            for y in (min.y / cell).floor() as i32..=(max.y / cell).floor() as i32 {
                grid.entry((&t.part_id, x, y)).or_default().push(i);
            }
        }
    }
    for indices in edges.values() {
        if let [a, b] = indices.as_slice() {
            let ra = root(&mut geometric_parent, *a);
            let rb = root(&mut geometric_parent, *b);
            geometric_parent[rb] = ra;
            let at = &map.triangles[*a].id;
            let bt = &map.triangles[*b].id;
            if at.source_mesh != bt.source_mesh
                || at.primitive != bt.primitive
                || at.source_triangle != bt.source_triangle
            {
                let ra = root(&mut parent, *a);
                let rb = root(&mut parent, *b);
                parent[rb] = ra;
            }
        }
    }
    for indices in source_edges.values() {
        if let [a, b] = indices.as_slice() {
            let ra = root(&mut parent, *a);
            let rb = root(&mut parent, *b);
            parent[rb] = ra;
        }
    }
    let ids: HashMap<_, _> = map
        .triangles
        .iter()
        .enumerate()
        .map(|(i, t)| (&t.id, i))
        .collect();
    let mut authored_same_part_links = 0;
    for c in map
        .connections
        .iter()
        .filter(|c| c.propagation && c.from.edge < 3 && c.to.edge < 3)
    {
        if let (Some(&a), Some(&b)) = (ids.get(&c.from.triangle), ids.get(&c.to.triangle)) {
            if map.triangles[a].part_id == map.triangles[b].part_id {
                let ra = root(&mut parent, a);
                let rb = root(&mut parent, b);
                parent[rb] = ra;
                authored_same_part_links += 1;
            }
        }
    }
    let source_edge_component_count = (0..parent.len())
        .map(|i| root(&mut parent, i))
        .collect::<BTreeSet<_>>()
        .len();
    // Independent source-plane membership check against raw stored world
    // coordinates. This expresses the original-face mask semantics without
    // relying on production's private graph or its restored metric arrays.
    let mut source_groups: HashMap<(&str, &str, u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in map.triangles.iter().enumerate() {
        source_groups
            .entry((
                &t.part_id,
                &t.id.source_mesh,
                t.id.primitive,
                t.id.source_triangle,
            ))
            .or_default()
            .push(i);
    }
    let normalized = |t: &PaintTriangle| {
        t.source_barycentric.map(|b| {
            let b = DVec3::from_array(b.map(f64::from));
            DVec2::new(b.y, b.z) / b.element_sum()
        })
    };
    let mut plane_links = 0;
    let mut plane_rejected = 0;
    let mut plane_rejected_details = Vec::new();
    let mut plane_collinear_details = Vec::new();
    let mut plane_groups_no_stable_reference = 0;
    let mut plane_max_corner_error_mm = 0_f64;
    for indices in source_groups.values().filter(|v| v.len() > 1) {
        let reference = indices
            .iter()
            .copied()
            .filter(|&i| {
                let w = world(&map.triangles[i]);
                let longest = (0..3)
                    .map(|e| w[e].distance(w[(e + 1) % 3]))
                    .fold(0., f64::max);
                (w[1] - w[0]).cross(w[2] - w[0]).length() / longest > 2e-6
            })
            .max_by(|&a, &b| {
                let det = |i| {
                    let b = normalized(&map.triangles[i]);
                    (b[1] - b[0]).perp_dot(b[2] - b[0]).abs()
                };
                det(a).total_cmp(&det(b))
            });
        let Some(reference) = reference else {
            plane_groups_no_stable_reference += 1;
            continue;
        };
        let s = normalized(&map.triangles[reference]);
        let source = DMat2::from_cols(s[1] - s[0], s[2] - s[0]);
        if source.determinant().abs() <= 1e-12 {
            plane_groups_no_stable_reference += 1;
            continue;
        }
        let w = world(&map.triangles[reference]);
        let physical = physical(w).unwrap() * source.inverse();
        let x = (w[1] - w[0]).normalize();
        let to_third = w[2] - w[0];
        let y = (to_third - x * to_third.dot(x)).normalize();
        for &i in indices {
            let q = normalized(&map.triangles[i]).map(|b| physical * (b - s[0]));
            let restored = q.map(|p| w[0] + (x * p.x + y * p.y) * 0.001);
            let error = restored
                .iter()
                .zip(world(&map.triangles[i]))
                .map(|(a, b)| a.distance(b))
                .fold(0., f64::max);
            if q.iter().all(|p| p.is_finite())
                && homogeneous_source_det(&map.triangles[i]) != 0.
                && error <= 5e-6
            {
                if (q[1] - q[0]).perp_dot(q[2] - q[0]) == 0. {
                    plane_collinear_details.push(json!({"triangle":map.triangles[i].id.stable_key(),"reference":map.triangles[reference].id.stable_key(),"partId":map.triangles[i].part_id,"homogeneousSourceDeterminant":homogeneous_source_det(&map.triangles[i]),"physicalDoubleAreaMm2":0.,"maxRawWorldCornerErrorMm":error*1000.}));
                }
                plane_max_corner_error_mm = plane_max_corner_error_mm.max(error * 1000.);
                let ra = root(&mut parent, reference);
                let rb = root(&mut parent, i);
                parent[rb] = ra;
                if i != reference {
                    plane_links += 1;
                }
            } else {
                plane_rejected += 1;
                plane_rejected_details.push(json!({"triangle":map.triangles[i].id.stable_key(),"reference":map.triangles[reference].id.stable_key(),"partId":map.triangles[i].part_id,"physicalDoubleAreaMm2":(q[1]-q[0]).perp_dot(q[2]-q[0]),"source":normalized(&map.triangles[i]).map(|p|p.to_array()),"physicalCoordinatesMm":q.map(|p|p.to_array()),"maxRawWorldCornerErrorMm":error*1000.}));
            }
        }
    }
    let geometric_count = (0..geometric_parent.len())
        .map(|i| root(&mut geometric_parent, i))
        .collect::<BTreeSet<_>>()
        .len();
    let mut components: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..parent.len() {
        let r = root(&mut parent, i);
        parent[i] = r;
        components.entry(r).or_default().push(i);
    }
    // Restore each source face's physical area scale from its largest stable
    // fragment. This counts source-positive microdomains even when stored f32
    // world corners become indistinguishable in one coordinate.
    let source_det = |t: &PaintTriangle| homogeneous_source_det(t).abs();
    let mut source_scales: HashMap<(&str, u32, u32), (f64, f64)> = HashMap::new();
    for t in &map.triangles {
        let w = world(t);
        let area = (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6;
        let det = source_det(t);
        if area > 0. && det > 0. {
            let entry = source_scales
                .entry((&t.id.source_mesh, t.id.primitive, t.id.source_triangle))
                .or_insert((0., 0.));
            if area > entry.0 {
                *entry = (area, area / det);
            }
        }
    }
    let source_area = |t: &PaintTriangle| {
        source_det(t)
            * source_scales
                .get(&(&*t.id.source_mesh, t.id.primitive, t.id.source_triangle))
                .map_or(0., |x| x.1)
    };
    let component_areas: HashMap<usize, f64> = components
        .iter()
        .map(|(r, indices)| {
            (
                *r,
                indices
                    .iter()
                    .map(|&i| source_area(&map.triangles[i]))
                    .sum(),
            )
        })
        .collect();
    let mut largest_part: HashMap<&str, usize> = HashMap::new();
    for (&r, indices) in &components {
        let part = map.triangles[indices[0]].part_id.as_str();
        let entry = largest_part.entry(part).or_insert(r);
        if component_areas[&r] > component_areas[entry] {
            *entry = r;
        }
    }
    let mut islands=components.iter().filter(|(r,indices)|largest_part[map.triangles[indices[0]].part_id.as_str()] != **r).map(|(&r,indices)|json!({"component":r,"partId":map.triangles[indices[0]].part_id,"triangles":indices.len(),"sourceAffineAreaMm2":component_areas[&r],"largestTriangle":indices.iter().max_by(|&&a,&&b|source_area(&map.triangles[a]).total_cmp(&source_area(&map.triangles[b]))).map(|&i|map.triangles[i].id.stable_key())})).collect::<Vec<_>>();
    islands.sort_by(|a, b| {
        b["sourceAffineAreaMm2"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["sourceAffineAreaMm2"].as_f64().unwrap())
    });
    let bins = [0., 1e-6, 1e-4, 0.01, 1., 100., f64::INFINITY];
    let island_bins=bins.windows(2).map(|b| {let rows=islands.iter().filter(|r| {let a=r["sourceAffineAreaMm2"].as_f64().unwrap();a>b[0] && a<=b[1]}).collect::<Vec<_>>();json!({"minExclusiveMm2":b[0],"maxInclusiveMm2":if b[1].is_finite(){Some(b[1])}else{None},"components":rows.len(),"areaMm2":rows.iter().map(|r|r["sourceAffineAreaMm2"].as_f64().unwrap()).sum::<f64>()})}).collect::<Vec<_>>();
    let mut by_part: BTreeMap<&str, Value> = BTreeMap::new();
    let mut identity_mismatches = Vec::new();
    let mut centroid_world_errors = Vec::new();
    let mut missing_centroids = Vec::new();
    let mut max_world_error = 0_f64;
    let mut lookup_missing = 0;
    for (i, t) in map.triangles.iter().enumerate() {
        let center = t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.;
        let candidates = grid.get(&(
            t.part_id.as_str(),
            (center.x / cell).floor() as i32,
            (center.y / cell).floor() as i32,
        ));
        let hits = candidates
            .into_iter()
            .flatten()
            .filter_map(|&j| {
                let b = barycentric_2d(center, map.triangles[j].paint_uv.map(Vec2::from_array))?;
                (b.min_element() >= -1e-5).then_some((j, b))
            })
            .collect::<Vec<_>>();
        let entry=by_part.entry(&t.part_id).or_insert_with(||json!({"triangles":0,"identityMismatches":0,"worldErrorAbove0_2Mm":0,"multipleCentroidHits":0,"lookupMissing":0,"components":0,"singleTriangleComponents":0,"largestComponentTriangles":0}));
        entry["triangles"] = json!(entry["triangles"].as_u64().unwrap() + 1);
        if hits.len() > 1 {
            entry["multipleCentroidHits"] =
                json!(entry["multipleCentroidHits"].as_u64().unwrap() + 1);
        }
        if let Some(&(j, b)) = hits.first() {
            let p = world(&map.triangles[j]);
            let recovered = p[0] * f64::from(b.x) + p[1] * f64::from(b.y) + p[2] * f64::from(b.z);
            let error = recovered.distance(centroid(t)) * 1000.;
            max_world_error = max_world_error.max(error);
            if error > 0.2 {
                entry["worldErrorAbove0_2Mm"] =
                    json!(entry["worldErrorAbove0_2Mm"].as_u64().unwrap() + 1);
                centroid_world_errors.push(json!({"partId":t.part_id,"sourceCenter":center.to_array(),"expectedTriangle":t.id.stable_key(),"locatedTriangle":map.triangles[j].id.stable_key(),"worldErrorMm":error,"sameTriangleLocated":i==j}));
            }
            if i != j {
                entry["identityMismatches"] =
                    json!(entry["identityMismatches"].as_u64().unwrap() + 1);
                identity_mismatches.push(json!({"partId":t.part_id,"sourceCenter":center.to_array(),"expectedTriangle":t.id.stable_key(),"locatedTriangle":map.triangles[j].id.stable_key(),"expectedChart":t.chart_id,"locatedChart":map.triangles[j].chart_id,"expectedWorld":centroid(t).to_array(),"locatedWorld":recovered.to_array(),"worldErrorMm":error,"centroidHits":hits.len()}));
            }
        } else {
            lookup_missing += 1;
            entry["lookupMissing"] = json!(entry["lookupMissing"].as_u64().unwrap() + 1);
            missing_centroids.push(json!({"partId":t.part_id,"triangle":t.id.stable_key(),"chartId":t.chart_id,"sourceCenter":center.to_array(),"sourceWorld":centroid(t).to_array(),"areaMm2":chart_metric(t).map_or(0.,|x|x.1)}));
        }
    }
    let mut anchors = Vec::new();
    for (indices_root, indices) in &components {
        let &index = indices
            .iter()
            .max_by(|&&a, &&b| {
                chart_metric(&map.triangles[a])
                    .map_or(0., |x| x.1)
                    .total_cmp(&chart_metric(&map.triangles[b]).map_or(0., |x| x.1))
            })
            .unwrap();
        let t = &map.triangles[index];
        let center = t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.;
        let located = topology.locate(&t.part_id, center);
        let same_component = located.is_some_and(|(j, _)| parent[j] == *indices_root);
        let coverage = topology.coverage_at(&t.part_id, center.to_array(), [300., 300.], 0., true);
        let source = coverage
            .iter()
            .filter(|f| f.part_id == t.part_id)
            .collect::<Vec<_>>();
        let entry = by_part.get_mut(t.part_id.as_str()).unwrap();
        entry["components"] = json!(entry["components"].as_u64().unwrap() + 1);
        if indices.len() == 1 {
            entry["singleTriangleComponents"] =
                json!(entry["singleTriangleComponents"].as_u64().unwrap() + 1);
        }
        entry["largestComponentTriangles"] = json!(
            entry["largestComponentTriangles"]
                .as_u64()
                .unwrap()
                .max(indices.len() as u64)
        );
        anchors.push(json!({"partId":t.part_id,"component":indices_root,"componentTriangles":indices.len(),"sourceTriangle":t.id.stable_key(),"sourceCenter":center.to_array(),"sourceWorld":centroid(t).to_array(),"sizeMm":[300.,300.],"rotationRad":0.,"locatedTriangle":located.map(|(j,_)|map.triangles[j].id.stable_key()),"sameComponentLocated":same_component,"fragments":coverage.len(),"sourceFragments":source.len(),"sourceAreaMm2":source.iter().map(|f|area(f)*1e6).sum::<f64>(),"componentAreaMm2":indices.iter().map(|&j|chart_metric(&map.triangles[j]).map_or(0.,|x|x.1)).sum::<f64>(),"sourceAffineComponentAreaMm2":component_areas[indices_root]}));
    }
    identity_mismatches.sort_by(|a, b| {
        b["worldErrorMm"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["worldErrorMm"].as_f64().unwrap())
    });
    let total_mismatches = identity_mismatches.len();
    identity_mismatches.truncate(1000);
    centroid_world_errors.sort_by(|a, b| {
        b["worldErrorMm"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["worldErrorMm"].as_f64().unwrap())
    });
    centroid_world_errors.truncate(1000);
    let mut edge_multiplicity: BTreeMap<&str, Value> = BTreeMap::new();
    for ((part, a, b), indices) in &edges {
        let entry = edge_multiplicity.entry(part).or_insert_with(||json!({"collapsedEdges":0,"moreThanTwoEntries":0,"moreThanTwoDistinctTriangles":0,"maxDistinctTriangles":0}));
        let distinct = indices.iter().copied().collect::<BTreeSet<_>>().len();
        if a == b {
            entry["collapsedEdges"] = json!(entry["collapsedEdges"].as_u64().unwrap() + 1);
        }
        if indices.len() > 2 {
            entry["moreThanTwoEntries"] = json!(entry["moreThanTwoEntries"].as_u64().unwrap() + 1);
        }
        if a != b && distinct > 2 {
            entry["moreThanTwoDistinctTriangles"] =
                json!(entry["moreThanTwoDistinctTriangles"].as_u64().unwrap() + 1);
        }
        entry["maxDistinctTriangles"] = json!(
            entry["maxDistinctTriangles"]
                .as_u64()
                .unwrap()
                .max(distinct as u64)
        );
    }
    json!({"spatialIndex":"64 chart-unit grid, original triangle order, same barycentric tolerance as public locate; component anchors are additionally verified through public locate", "triangleCentroidsChecked":map.triangles.len(),"identityMismatches":total_mismatches,"lookupMissing":lookup_missing,"maxWorldErrorMm":max_world_error,"byPart":by_part,"worstIdentityMismatches":identity_mismatches,"worldErrorsAbove0_2Mm":centroid_world_errors,"missingCentroids":missing_centroids,"componentGraph":{"definition":"Exactly-two-entry source edges keyed by part/sourceMesh/primitive/sourceFace and exact f64 bits of normalized (b1,b2)/sum(b); exactly-two-entry 1um spatial edges link only different source identities; valid authored same-part propagation links included. Additionally, retained fragments of one original source face+part share a stable affine plane even across mask holes; independent raw-world reconstruction requires reference altitude >2um, normalized source determinant >1e-12, nonzero homogeneous barycentric source determinant (including positive domains whose normalized 2D differences cancel to collinearity) and all corner residuals <=5um. This group relation extends image coordinates to existing masks only, never generates a gap triangle. Production private links/metric arrays are not read.","sourceAwareComponents":components.len(),"sourceAwareEdgeComponentsBeforePlaneGroups":source_edge_component_count,"legacyQuantizedGeometricComponents":geometric_count,"authoredSamePartLinks":authored_same_part_links,"sourcePlaneLinks":plane_links,"sourcePlaneRejectedFragments":plane_rejected,"sourcePlaneRejectedDetails":plane_rejected_details,"sourcePlaneAcceptedCollinearDetails":plane_collinear_details,"sourcePlaneGroupsWithoutStableReference":plane_groups_no_stable_reference,"sourcePlaneMaxRawWorldCornerErrorMm":plane_max_corner_error_mm,"islandDefinition":"All components except the largest source-affine-area component per part. Area uses normalized source domain scaled by largest stable stored-world fragment on that original source face; no positive source domain is dropped by an area epsilon.","islands":islands.len(),"positiveAreaIslands":islands.iter().filter(|r|r["sourceAffineAreaMm2"].as_f64().unwrap()>0.).count(),"areaBins":island_bins,"islandDetails":islands},"componentAnchorCoverage":anchors,"geometricEdgeMultiplicity":edge_multiplicity,"tJunctions":t_junction_report(&edges,&parent,&component_areas,map)})
}

fn sample(t: &PaintTriangle, edge: usize, fraction: f64) -> (Vec2, DVec3) {
    let mut bary = [0.; 3];
    bary[edge] = (1. - fraction) * 0.9;
    bary[(edge + 1) % 3] = fraction * 0.9;
    bary[(edge + 2) % 3] = 0.1;
    let uv = t.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
    let w = world(t);
    (
        (uv[0] * bary[0] + uv[1] * bary[1] + uv[2] * bary[2]).as_vec2(),
        w[0] * bary[0] + w[1] * bary[1] + w[2] * bary[2],
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut map_path = std::env::var_os("ITASHA_TEST_PAINT_MAP").map(PathBuf::from);
    let mut output = PathBuf::from(".reports/seam-metric-audit.json");
    let mut mode = "stratified".to_string();
    let mut limit = usize::MAX;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--map" => map_path = Some(args.next().ok_or("--map path")?.into()),
            "--output" => output = args.next().ok_or("--output path")?.into(),
            "--mode" => mode = args.next().ok_or("--mode full|stratified")?,
            "--limit" => limit = args.next().ok_or("--limit number")?.parse()?,
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }
    let map_path = if let Some(path) = map_path {
        path
    } else {
        let paint_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/paint");
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(paint_root.join("manifest.json"))?)?;
        paint_root.join(
            manifest["runtimePaint"]
                .as_str()
                .ok_or("manifest runtimePaint path")?,
        )
    };
    let start = Instant::now();
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&map_path)?)?;
    map.validate()?;
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let build_ms = start.elapsed().as_secs_f64() * 1000.;
    let triangles: HashMap<_, _> = map.triangles.iter().map(|t| (&t.id, t)).collect();
    let mut pairs: BTreeMap<(&str, &str), Vec<&EdgeConnection>> = BTreeMap::new();
    for c in map.connections.iter().filter(|c| c.propagation) {
        let a = triangles[&c.from.triangle].part_id.as_str();
        let b = triangles[&c.to.triangle].part_id.as_str();
        if a != b {
            pairs.entry((a.min(b), a.max(b))).or_default().push(c);
        }
    }
    let mut representatives = BTreeSet::new();
    let mut representative_seeds = Vec::new();
    for connections in pairs.values_mut() {
        let midpoint = |c: &EdgeConnection| {
            (centroid(triangles[&c.from.triangle]) + centroid(triangles[&c.to.triangle])) * 0.5
        };
        let min = connections
            .iter()
            .map(|c| midpoint(c))
            .fold(DVec3::splat(f64::INFINITY), DVec3::min);
        let max = connections
            .iter()
            .map(|c| midpoint(c))
            .fold(DVec3::splat(f64::NEG_INFINITY), DVec3::max);
        let span = max - min;
        let axis = (0..3).max_by(|&a, &b| span[a].total_cmp(&span[b])).unwrap();
        connections.sort_by(|a, b| {
            midpoint(a)[axis]
                .total_cmp(&midpoint(b)[axis])
                .then(a.id.cmp(&b.id))
        });
        for i in [
            0,
            connections.len() / 6,
            connections.len() / 3,
            connections.len() / 2,
            connections.len() * 2 / 3,
            connections.len() * 5 / 6,
            connections.len() - 1,
        ] {
            representatives.insert(connections[i].id.as_str());
        }
    }
    let mut rows = Vec::new();
    let mut summary: BTreeMap<String, Value> = BTreeMap::new();
    let mut query_ms = Vec::new();
    'pairs: for ((a, b), connections) in &pairs {
        for c in connections {
            let representative = representatives.contains(c.id.as_str());
            if mode != "full" && !representative {
                continue;
            }
            for (direction, source_ref, target_ref) in
                [("forward", &c.from, &c.to), ("reverse", &c.to, &c.from)]
            {
                let source = triangles[&source_ref.triangle];
                let target = triangles[&target_ref.triangle];
                for fraction in [0.2, 0.5, 0.8] {
                    let (center, seed_world) = sample(source, source_ref.edge as usize, fraction);
                    let cases: &[(&str, [f32; 2], f32)] = if representative && fraction == 0.5 {
                        &[
                            ("small-square", [300., 300.], 0.17),
                            ("wide-rotated", [700., 230.], 0.73),
                            ("tall-rotated", [230., 700.], -1.13),
                            ("large-square", [700., 700.], 0.),
                        ]
                    } else {
                        &[("small-square", [300., 300.], 0.17)]
                    };
                    for &(case, size, rotation) in cases {
                        if rows.len() >= limit {
                            break 'pairs;
                        }
                        let now = Instant::now();
                        let coverage = topology.coverage_at(
                            &source.part_id,
                            center.to_array(),
                            size,
                            rotation,
                            true,
                        );
                        let elapsed = now.elapsed().as_secs_f64() * 1000.;
                        query_ms.push(elapsed);
                        let single = topology.coverage_at(
                            &source.part_id,
                            center.to_array(),
                            size,
                            rotation,
                            false,
                        );
                        let source_cross = coverage
                            .iter()
                            .filter(|f| f.part_id == source.part_id)
                            .cloned()
                            .collect::<Vec<_>>();
                        let mut metrics = Vec::new();
                        let mut metric_area = 0.;
                        let mut total_area = 0.;
                        let mut target_area = 0.;
                        let mut local_area = 0.;
                        let mut target_seed_area = 0.;
                        let mut finite = true;
                        let mut opposite_area = 0.;
                        let mut parts = BTreeSet::new();
                        for f in &coverage {
                            let weight = area(f);
                            total_area += weight;
                            parts.insert(f.part_id.as_str());
                            finite &= f
                                .image_uv
                                .iter()
                                .flatten()
                                .all(|v| v.is_finite() && (-0.00001..=1.00001).contains(v));
                            if f.part_id == target.part_id {
                                target_area += weight;
                                if (f
                                    .world
                                    .iter()
                                    .map(|p| DVec3::from_array(p.map(f64::from)))
                                    .sum::<DVec3>()
                                    / 3.)
                                    .distance(seed_world)
                                    < 0.25
                                {
                                    local_area += weight;
                                }
                                if f.triangle == target.id {
                                    target_seed_area += weight;
                                }
                            }
                            if (source.part_id.contains("-left-") && f.part_id.contains("-right-"))
                                || (source.part_id.contains("-right-")
                                    && f.part_id.contains("-left-"))
                            {
                                opposite_area += weight;
                            }
                            if let Some((s, _, _)) = fragment_metric(f, size) {
                                metric_area += weight;
                                metrics.push(((s[0] - 1.).abs().max((s[1] - 1.).abs()), weight));
                            }
                        }
                        let error_p95 = weighted(&metrics, 0.95);
                        let error_max = metrics.iter().map(|x| x.0).fold(0., f64::max);
                        let located = topology
                            .locate(&source.part_id, center)
                            .map(|(i, _)| topology.triangles[i].id.stable_key());
                        let source_unchanged = single == source_cross;
                        let success = target_area > 1e-7 && local_area > 1e-7;
                        let witness = direction_witness(&coverage, source, target, size);
                        let mut row = json!({"seamId":c.id,"pair":[a,b],"direction":direction,"case":case,"edgeFraction":fraction,"source":source.part_id,"target":target.part_id,"sourceTriangle":source.id.stable_key(),"targetTriangle":target.id.stable_key(),"locatedTriangle":located,"sourceCenter":center.to_array(),"sourceWorld":seed_world.to_array(),"sizeMm":size,"rotationRad":rotation,"physicalGapMm":c.physical_gap_mm,"fragments":coverage.len(),"coveredParts":parts,"targetAreaMm2":target_area*1e6,"targetLocalAreaMm2":local_area*1e6,"targetSeedTriangleAreaMm2":target_seed_area*1e6,"crossingSucceeded":success,"sourceUnchanged":source_unchanged,"finiteImageUv":finite,"oppositeSideAreaMm2":opposite_area*1e6,"metricMeasuredAreaFraction":if total_area>0.{metric_area/total_area}else{0.},"physicalJacobianSingularErrorP95":error_p95,"physicalJacobianSingularErrorMax":error_max,"directionWitness":witness,"queryMs":elapsed});
                        let exterior = exterior_witness(&coverage, source, target, size);
                        let exterior_usable = exterior["usable"].as_bool() == Some(true);
                        let exterior_differing =
                            exterior["sameDominantHandedness"].as_bool() == Some(false);
                        row["exteriorDirectionWitness"] = exterior;
                        row["sourceAreaMm2"] =
                            json!(source_cross.iter().map(|f| area(f) * 1e6).sum::<f64>());
                        row["sourceFragments"] = json!(source_cross.len());
                        row["totalAreaMm2"] = json!(total_area * 1e6);
                        if representative && fraction == 0.5 && case == "small-square" {
                            representative_seeds.push(row.clone());
                        }
                        let key = format!("{a} / {b}");
                        let entry=summary.entry(key).or_insert_with(||json!({"queries":0,"crossingFailures":0,"sourceChanges":0,"nonFinite":0,"metricP95Above1Percent":0,"smallOppositeSideJumps":0,"exteriorDirectionUsable":0,"exteriorDirectionUnusable":0,"exteriorDirectionDifferingHandedness":0,"maxMetricP95":0.0}));
                        for (key, condition) in [
                            ("queries", true),
                            ("crossingFailures", !success),
                            ("sourceChanges", !source_unchanged),
                            ("nonFinite", !finite),
                            ("metricP95Above1Percent", error_p95 > 0.01),
                            ("exteriorDirectionUsable", exterior_usable),
                            ("exteriorDirectionUnusable", !exterior_usable),
                            ("exteriorDirectionDifferingHandedness", exterior_differing),
                            (
                                "smallOppositeSideJumps",
                                case == "small-square" && opposite_area > 1e-7,
                            ),
                        ] {
                            if condition {
                                entry[key] = json!(entry[key].as_u64().unwrap() + 1);
                            }
                        }
                        entry["maxMetricP95"] =
                            json!(entry["maxMetricP95"].as_f64().unwrap().max(error_p95));
                        rows.push(row);
                        if rows.len() % 1000 == 0 {
                            eprintln!(
                                "audited {} queries in {:.1}s",
                                rows.len(),
                                start.elapsed().as_secs_f64()
                            );
                        }
                    }
                }
            }
        }
    }
    query_ms.sort_by(f64::total_cmp);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut report = json!({"schema":"itasha-seam-metric-audit-v1","mapPath":map_path,"paintFingerprint":map.paint_fingerprint,"mode":mode,"triangles":map.triangles.len(),"authoredSeams":pairs.values().map(Vec::len).sum::<usize>(),"physicalPartPairs":pairs.len(),"queries":rows.len(),"topologyBuildMs":build_ms,"elapsedSeconds":start.elapsed().as_secs_f64(),"queryP50Ms":query_ms.get(query_ms.len()/2),"queryP95Ms":query_ms.get(query_ms.len()*95/100),"sampling":"every seam: 0.2/0.5/0.8 edge position, 10% inside source triangle, both directions; 7 spatial strata/pair add wide+tall rotations and 700mm square","metricDefinition":"singular values of generated image_uv*sizeMm differentiated against intrinsic physical world millimetres; expected [1,1]. Thin clipped output fragments excluded from numerical derivative only; coverage areas retain them.","mirrorScope":"coverage geometry has no artwork mirror parameter; x/y UV reflection leaves singular values invariant. Packing reflection is exercised by metric_propagation integration tests; renderer mirroring requires GPU acceptance.","chartMetric":chart_report(&map),"summaryByPair":summary,"representativeSeeds":representative_seeds,"results":rows});
    let count_exterior = |field: &str, expected: bool| {
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["exteriorDirectionWitness"][field].as_bool() == Some(expected))
            .count()
    };
    report["exteriorDirectionSummary"] = json!({
        "usable": count_exterior("usable", true),
        "unusable": count_exterior("usable", false),
        "sameHandedness": count_exterior("sameDominantHandedness", true),
        "differingHandedness": count_exterior("sameDominantHandedness", false),
        "definition": "Unusable witnesses remain in the query count and do not certify exterior direction. Coverage success alone does not certify source connected area or complete paint geometry."
    });
    eprintln!("auditing all triangle centroids and component anchors");
    report["anchors"] = anchor_audit(&map, &topology);
    report["elapsedSeconds"] = json!(start.elapsed().as_secs_f64());
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    eprintln!(
        "saved {}: {} queries, {:.1}s",
        output.display(),
        report["queries"],
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
