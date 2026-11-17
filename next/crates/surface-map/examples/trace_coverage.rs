//! Diagnostic for the lower-fender coverage regression, using the runtime map.
use itasha_surface_map::{PaintSurfaceMap, topology::Topology};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("assets/paint/manifest.json")).expect("paint manifest"),
        )
        .expect("paint manifest JSON");
        root.join("assets/paint")
            .join(
                manifest["runtimePaint"]
                    .as_str()
                    .expect("runtime paint path"),
            )
            .to_string_lossy()
            .into_owned()
    }));
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(path)?)?;
    map.validate()?;
    let triangles: std::collections::HashMap<_, _> =
        map.triangles.iter().map(|t| (t.id.clone(), t)).collect();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    for c in &map.connections {
        let a = &triangles[&c.from.triangle];
        let b = &triangles[&c.to.triangle];
        if !(a.part_id.ends_with("lower-fender") || b.part_id.ends_with("lower-fender")) {
            continue;
        }
        let pair_part = if a.part_id.ends_with("front-left-lower-fender") {
            a.part_id.clone()
        } else if b.part_id.ends_with("front-left-lower-fender") {
            b.part_id.clone()
        } else {
            continue;
        };
        let _ = pair_part;
        println!("seam {} parts {} -> {}", c.id, a.part_id, b.part_id);
        for (side, source, target) in [("from", a, b), ("to", b, a)] {
            let center = source
                .paint_uv
                .map(glam::Vec2::from_array)
                .into_iter()
                .sum::<glam::Vec2>()
                / 3.;
            for size in [300.0_f32, 700.0] {
                let coverage = topology.coverage_at(
                    &source.part_id,
                    center.to_array(),
                    [size, size],
                    0.17,
                    true,
                );
                let bad: Vec<_> = coverage
                    .iter()
                    .filter(|f| f.part_id.contains("-right-") && source.part_id.contains("-left-"))
                    .map(|f| f.part_id.clone())
                    .collect();
                let mut parts: std::collections::BTreeMap<&str, usize> =
                    std::collections::BTreeMap::new();
                for f in &coverage {
                    *parts.entry(f.part_id.as_str()).or_default() += 1;
                }
                println!(
                    "  [{side}] seed {} size {size}: {} frags {:?} bad {}",
                    source.part_id,
                    coverage.len(),
                    parts,
                    bad.len()
                );
                if !bad.is_empty() {
                    println!("    seam {} bad parts {:?}", c.id, {
                        let mut counts: std::collections::BTreeMap<&str, usize> =
                            std::collections::BTreeMap::new();
                        for f in coverage.iter().filter(|f| {
                            f.part_id.contains("-right-") && source.part_id.contains("-left-")
                        }) {
                            *counts.entry(f.part_id.as_str()).or_default() += 1;
                        }
                        counts
                    });
                    println!(
                        "    seed triangle {} world {:?} uv {:?} chart {}",
                        source.id.stable_key(),
                        source.world,
                        source.paint_uv,
                        source.chart_id
                    );
                    let sc = source
                        .world
                        .iter()
                        .fold(glam::Vec3::ZERO, |a, p| a + glam::Vec3::from_array(*p))
                        / 3.;
                    let mut far: Vec<_> = coverage
                        .iter()
                        .filter(|f| f.part_id.contains("-right-"))
                        .map(|f| {
                            let c = f
                                .world
                                .iter()
                                .fold(glam::Vec3::ZERO, |a, p| a + glam::Vec3::from_array(*p))
                                / 3.;
                            (c.distance(sc), f.part_id.clone())
                        })
                        .collect();
                    far.sort_by(|a, b| a.0.total_cmp(&b.0));
                    println!(
                        "    nearest bad {} {:.3} m, farthest bad {} {:.3} m",
                        far.first().map(|f| f.1.as_str()).unwrap_or(""),
                        far.first().map(|f| f.0).unwrap_or(0.),
                        far.last().map(|f| f.1.as_str()).unwrap_or(""),
                        far.last().map(|f| f.0).unwrap_or(0.)
                    );
                    let _ = &bad;
                }
            }
            let target_center = target
                .paint_uv
                .map(glam::Vec2::from_array)
                .into_iter()
                .sum::<glam::Vec2>()
                / 3.;
            let _ = target_center;
        }
    }
    Ok(())
}
