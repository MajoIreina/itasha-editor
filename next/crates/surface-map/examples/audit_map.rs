use glam::Vec2;
use itasha_surface_map::{CoverageRect, PaintSurfaceMap, topology::Topology};
use serde_json::json;
use std::time::Instant;
fn main() {
    let path = std::env::args().nth(1).expect("map path");
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    map.validate().unwrap();
    let start = Instant::now();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let build_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut parts: Vec<_> = map.triangles.iter().map(|t| t.part_id.clone()).collect();
    parts.sort();
    parts.dedup();
    let mut report = vec![];
    for part in parts {
        let triangle = map
            .triangles
            .iter()
            .filter(|t| t.part_id == part)
            .max_by(|a, b| {
                let area = |t: &itasha_surface_map::PaintTriangle| {
                    let p = t.world.map(glam::Vec3::from_array);
                    (p[1] - p[0]).cross(p[2] - p[0]).length()
                };
                area(a).total_cmp(&area(b))
            })
            .unwrap();
        let uv = triangle.paint_uv.map(Vec2::from_array);
        let center = (uv[0] + uv[1] + uv[2]) / 3.;
        let mut times = vec![];
        let mut count = 0;
        let mut covered_parts = vec![];
        for _ in 0..20 {
            let start = Instant::now();
            let coverage = topology.coverage(
                &part,
                CoverageRect {
                    center,
                    size: Vec2::splat(300.),
                    rotation_rad: 0.,
                },
                true,
            );
            times.push(start.elapsed().as_secs_f64() * 1000.);
            count = coverage.len();
            covered_parts = coverage.iter().map(|f| f.part_id.clone()).collect();
        }
        times.sort_by(f64::total_cmp);
        covered_parts.sort();
        covered_parts.dedup();
        report.push(json!({"partId":part,"seed":center.to_array(),"fragments":count,"coverageComputeP50Ms":times[10],"coverageComputeP95Ms":times[18],"coveredParts":covered_parts}));
    }
    println!("{}",serde_json::to_string_pretty(&json!({"topologyBuildMs":build_ms,"triangles":map.triangles.len(),"connections":map.connections.len(),"samplesPerPart":20,"imageMm":[300,300],"parts":report})).unwrap());
}
