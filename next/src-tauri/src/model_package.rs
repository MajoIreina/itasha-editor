//! SU7 asset adapter. Rendering stays model-neutral in itasha-renderer.
use itasha_contracts::{AppError, AppResult, SessionRef};
use itasha_renderer::assets::{self, Paint, Plan};
use serde_json::Value;
use std::path::PathBuf;

/// Superseded maps that upgrade by fingerprint rebinding: v21 (classified-only),
/// v22 (whole-car seams) and v23 (restored parts). The v24 mirror keeps every
/// triangle except the replaced `su7.front-left-lower-fender` part, whose chart
/// layout changed, so layers on that part keep their 3D placement but are drawn
/// in the mirrored chart.
pub const PREVIOUS_PAINTS: &[&str] = &[
    "b16c24a5ff3ba086c545841dd92fd5d0a99ea50ab0cd270b94caca9d8b767db1",
    "cc2835d5d4035ef0e4541ef4d9f5fd2da5f671b9c0808a0538843c497a87f671",
    "5528f079561b12cb3dee51c495f6507ad7f58a291902afa0583353064b5c52e3",
];
pub const CURRENT_PAINT: &str = "109ba0aa146625f99b35050b2682edb6c0ad15b72e180c515c58867fa7d275d7";

/// Each predecessor may upgrade because every one of its triangles survives
/// unchanged in the current map; only missing parts and faces were appended.
/// The surface-map regression test proves the superset relation.
pub fn verified_seam_upgrade(plan: &Plan, fingerprint: Option<&str>) -> AppResult<bool> {
    if !fingerprint.is_some_and(|value| PREVIOUS_PAINTS.contains(&value)) {
        return Ok(false);
    }
    let Some(path) = &plan.paint_map else {
        return Ok(false);
    };
    let map: itasha_surface_map::PaintSurfaceMap =
        serde_json::from_slice(&std::fs::read(path).map_err(AppError::io)?)
            .map_err(AppError::io)?;
    if map.paint_fingerprint != CURRENT_PAINT {
        return Ok(false);
    }
    map.validate()
        .map_err(|e| AppError::new("model-mismatch", e))?;
    Ok(true)
}

fn resolve_root(configured: &str) -> AppResult<PathBuf> {
    let mut candidates = vec![];
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        candidates.extend([
            path.clone(),
            path.join("Blender/software"),
            path.join("software"),
        ]);
    } else if let Ok(exe) = std::env::current_exe() {
        for base in exe.ancestors().skip(1) {
            candidates.push(base.join("3D/Blender/software"));
            candidates.push(base.join("models/su7"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join("config.json").is_file() && p.join("body.glb").is_file())
        .ok_or_else(|| {
            AppError::new(
                "model-missing",
                "未找到处理后的模型包。请在设置的模型资源目录选择 3D 或 Blender/software。",
            )
        })
}
fn index(id: &str, ids: &[&str]) -> AppResult<usize> {
    ids.iter()
        .position(|v| *v == id)
        .ok_or_else(|| AppError::new("model-configuration", "当前选项尚无模型资产映射。"))
}
pub fn plan(view: &itasha_application::View, session: &SessionRef) -> AppResult<Plan> {
    let project = view
        .project
        .as_ref()
        .ok_or_else(|| AppError::new("no-project", "请先打开设计。"))?;
    if project.session.project_id != session.project_id
        || project.session.session_epoch != session.session_epoch
    {
        return Err(AppError::new("stale-session", "车型加载会话已过期。"));
    }
    if project.vehicle_id.as_deref() != Some("su7") {
        return Err(AppError::new(
            "model-unavailable",
            "当前只接入 SU7 Max 模块化资产；此车型的专用模型尚未接入。",
        ));
    }
    let configuration = project
        .configuration
        .as_ref()
        .ok_or_else(|| AppError::new("model-configuration", "工程缺少车型选配。"))?;
    let root = resolve_root(&view.settings.paths.models)?;
    let config_file = assets::package_file(&root, "config.json")
        .map_err(|e| AppError::new("model-package", &e))?;
    if config_file.metadata().map_err(AppError::io)?.len() > 1024 * 1024 {
        return Err(AppError::new("model-package", "模型配置文件过大。"));
    }
    let config: Value = serde_json::from_slice(&std::fs::read(config_file).map_err(AppError::io)?)
        .map_err(AppError::io)?;
    let paint = index(
        &configuration.paint,
        &[
            "600061067",
            "600061064",
            "600061061",
            "600061068",
            "600061066",
            "600061059",
            "600061062",
            "600061063",
        ],
    )?;
    let wheel = index(
        &configuration.wheels,
        &[
            "600061079",
            "600061080",
            "600061081",
            "600061084",
            "600138252",
        ],
    )?;
    let interior = index(
        &configuration.interior,
        &[
            "600061091",
            "unused-CobaltBlue",
            "600061088",
            "600061089",
            "600061087",
        ],
    )?;
    let steering = index(&configuration.steering, &["600061069", "600061070"])?;
    let invalid = || AppError::new("model-package", "模型配置结构与适配器不兼容。");
    if config["version"] != 1 {
        return Err(invalid());
    }
    let p = &config["paint"][paint];
    let color: [f32; 4] =
        serde_json::from_value(p["base_color_linear"].clone()).map_err(|_| invalid())?;
    if !color
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    {
        return Err(invalid());
    }
    let scalar = |v: &Value, default: f32| {
        v.as_f64()
            .map(|n| n as f32)
            .unwrap_or(default)
            .clamp(0., 1.)
    };
    Ok(Plan {
        paint_map: Some({
            let relative = "assets/paint/su7-v24-mirrored/paint-map.json";
            let mut candidates = vec![root.join("paint/paint-map.json")];
            if let Ok(exe) = std::env::current_exe() {
                for parent in exe.ancestors().skip(1) {
                    candidates.push(parent.join(relative));
                }
            }
            candidates.push(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join(relative),
            );
            candidates
                .into_iter()
                .find(|p| p.is_file())
                .ok_or_else(|| {
                    AppError::new(
                        "paint-map-missing",
                        "缺少新版漆面展开资产，请将 assets/paint 随程序一起部署。",
                    )
                })?
        }),
        surfaces: itasha_contracts::SU7_SURFACES
            .iter()
            // These source nodes lack user-classified exterior paint masks and
            // must not be populated from legacy UV guesses. Trunk paint already
            // comes from the reviewed v20c lineage carried forward by v23.
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
            config["body_asset"].as_str().ok_or_else(invalid)?.into(),
            config["wheel"][wheel]["asset"]
                .as_str()
                .ok_or_else(invalid)?
                .into(),
            config["steering"][steering]["asset"]
                .as_str()
                .ok_or_else(invalid)?
                .into(),
        ],
        paint: Some(Paint {
            color,
            metallic: scalar(&p["original_parameters"]["params"]["metalness"], 0.7),
            roughness: scalar(&p["pbr_roughness"], 0.29),
            coat: scalar(&p["pbr_clearcoat"], 1.),
        }),
        interior: Some(
            config["interior"][interior]["id"]
                .as_str()
                .ok_or_else(invalid)?
                .into(),
        ),
        texture_limit: u32::from(view.settings.performance.texture_limit).min(4096),
    })
}
