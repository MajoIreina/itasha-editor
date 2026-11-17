//! Offline runtime check: load the real SU7 package with a paint map and list
//! every editable surface. Used to prove a restored map (e.g. su7-v23) reaches
//! the 2D canvas without starting the desktop shell.
//!
//! cargo run -p itasha-renderer --example verify_paint_package -- L:/SU7/3D/Blender/software assets/paint/su7-v23-restored/paint-map.json
use itasha_renderer::assets::{self, Plan};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let map = PathBuf::from(&args[2]);
    let cfg: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("config.json"))?)?;
    let scene = assets::load(&Plan {
        paint_map: Some(map),
        surfaces: itasha_contracts::SU7_SURFACES
            .iter()
            .filter(|s| {
                !matches!(
                    s.id,
                    "su7.rear-trim" | "su7.cowl" | "su7.left-sill" | "su7.right-sill"
                )
            })
            .map(|s| itasha_renderer::surface::SurfacePlan {
                id: s.id.into(),
                name: s.name.into(),
                mesh_id: s.mesh_id.into(),
                node_name: s.node_name.into(),
                region_bounds: s.region_bounds,
                whole_components: s.whole_components,
                u: s.u,
                v: s.v,
                normal: s.normal,
            })
            .collect(),
        root,
        modules: vec![
            "body.glb".into(),
            cfg["wheel"][0]["asset"].as_str().ok_or("wheel")?.into(),
            cfg["steering"][0]["asset"]
                .as_str()
                .ok_or("steering")?
                .into(),
        ],
        paint: None,
        interior: None,
        texture_limit: 256,
    })?;
    println!("fingerprint {}", scene.stats.surface_fingerprint);
    println!("surfaces {}", scene.surfaces.len());
    for surface in &scene.surfaces {
        println!(
            "  {} tris={} size={:.0}x{:.0}mm",
            surface.info.id, surface.info.triangles, surface.info.width_mm, surface.info.height_mm
        );
    }
    Ok(())
}
