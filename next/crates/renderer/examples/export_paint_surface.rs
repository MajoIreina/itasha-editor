//! Offline input for Blender unwrap; original model assets are read-only.
use itasha_renderer::{
    assets::{self, Plan},
    surface::SurfacePlan,
};
use itasha_surface_map::{MAP_VERSION, PaintSurfaceMap};
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(
        args.get(1)
            .ok_or("usage: export_paint_surface model-root new-output.json")?,
    );
    let output = PathBuf::from(args.get(2).ok_or("missing output")?);
    let cfg: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("config.json"))?)?;
    let scene = assets::load(&Plan {
        paint_map: None,
        root: root.clone(),
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
        surfaces: itasha_contracts::SU7_SURFACES
            .iter()
            .filter(|s| !matches!(s.id, "su7.rear-trim" | "su7.cowl"))
            .map(|s| SurfacePlan {
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
    })?;
    let mut map = PaintSurfaceMap {
        mapping_version: MAP_VERSION,
        model_fingerprint: format!(
            "{:x}",
            Sha256::digest(std::fs::read(root.join("body.glb"))?)
        ),
        paint_fingerprint: String::new(),
        triangles: scene
            .surfaces
            .iter()
            .flat_map(|s| s.editable_triangles.clone())
            .collect(),
        connections: vec![],
    };
    map.paint_fingerprint = map.fingerprint();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    file.write_all(&serde_json::to_vec(&map)?)?;
    eprintln!(
        "Exported {} candidates to {}; provisional runtime partition, requires semantic review and unwrap",
        map.triangles.len(),
        output.display()
    );
    Ok(())
}
