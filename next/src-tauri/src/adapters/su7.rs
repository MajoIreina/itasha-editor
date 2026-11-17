//! SU7 asset adapter. Rendering stays model-neutral in itasha-renderer.
use itasha_contracts::{AppError, AppResult, SessionRef};
use itasha_renderer::assets::{self, Paint, Plan};
use serde_json::Value;
use std::path::PathBuf;

/// A changed chart requires source-barycentric reprojection, including history.
/// Never claim that a fingerprint-only rebind preserves saved placement.
const PREVIOUS_PAINTS: &[(&str, &str)] = &[
    (
        "b16c24a5ff3ba086c545841dd92fd5d0a99ea50ab0cd270b94caca9d8b767db1",
        "su7-v21-classified-only/paint-map.json",
    ),
    (
        "cc2835d5d4035ef0e4541ef4d9f5fd2da5f671b9c0808a0538843c497a87f671",
        "su7-v22-whole-car/paint-map.json",
    ),
    (
        "5528f079561b12cb3dee51c495f6507ad7f58a291902afa0583353064b5c52e3",
        "su7-v23-restored/paint-map.json",
    ),
    (
        "109ba0aa146625f99b35050b2682edb6c0ad15b72e180c515c58867fa7d275d7",
        "su7-v24-mirrored/paint-map.json",
    ),
];

fn manifest() -> &'static Value {
    static MANIFEST: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/paint/manifest.json"))
            .expect("bundled paint manifest")
    })
}

fn paint_candidates(relative: &str) -> Vec<PathBuf> {
    let relative = format!("assets/paint/{relative}");
    let mut candidates = vec![];
    if let Ok(exe) = std::env::current_exe() {
        for parent in exe.ancestors().skip(1) {
            candidates.push(parent.join(&relative));
        }
    }
    candidates.push(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(relative),
    );
    candidates
}

pub fn upgrade_mapping(
    app: &mut itasha_application::Application,
    plan: &Plan,
    session: &SessionRef,
    fingerprint: &str,
) -> AppResult<bool> {
    let current = manifest()["paintFingerprint"]
        .as_str()
        .expect("paint fingerprint");
    if fingerprint == current {
        return Ok(false);
    }
    let Some((_, predecessor)) = PREVIOUS_PAINTS
        .iter()
        .find(|(hash, _)| *hash == fingerprint)
    else {
        return Ok(false);
    };
    let Some(path) = &plan.paint_map else {
        return Ok(false);
    };
    let map: itasha_surface_map::PaintSurfaceMap =
        serde_json::from_slice(&std::fs::read(path).map_err(AppError::io)?)
            .map_err(AppError::io)?;
    if map.paint_fingerprint != current {
        return Ok(false);
    }
    let old_path = paint_candidates(predecessor)
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            AppError::new(
                "paint-migration-unavailable",
                "此工程需要对应旧漆面地图才能安全迁移，请恢复原映射包后重试。",
            )
        })?;
    let old: itasha_surface_map::PaintSurfaceMap =
        serde_json::from_slice(&std::fs::read(old_path).map_err(AppError::io)?)
            .map_err(AppError::io)?;
    if old.paint_fingerprint != fingerprint {
        return Err(AppError::new("model-mismatch", "旧漆面地图指纹不匹配。"));
    }
    // This original face is physically on the front-left fender. Older masks
    // also classified two overlapping fragments as rear-bumper paint.
    let corrections = [itasha_surface_map::transition::SourcePartCorrection {
        source_mesh: "body.glb/node/175/primitive/0",
        primitive: 0,
        source_triangle: 32590,
        from_part: "su7.rear-bumper",
        to_part: "su7.front-left-fender",
    }];
    let transition = itasha_surface_map::transition::MappingTransition::with_part_corrections(
        &old,
        &map,
        &corrections,
    )
    .map_err(|e| AppError::new("model-mismatch", e))?;
    app.remap_surface_mapping(session, fingerprint, current, |layer| {
        let p = &mut layer.placement;
        let anchor = p.anchor.as_ref().ok_or_else(|| {
            AppError::new("paint-migration-unavailable", "旧图层缺少可迁移锚点。")
        })?;
        if anchor.paint_surface_fingerprint != fingerprint {
            return Err(AppError::new("model-mismatch", "旧图层锚点指纹不匹配。"));
        }
        let mapped = transition
            .reproject(
                &anchor.source_triangle_id,
                &p.surface_id,
                anchor.barycentric.into(),
                (p.rotation_deg as f32).to_radians(),
            )
            .map_err(|e| {
                AppError::new(
                    "paint-migration-unavailable",
                    format!("图层“{}”无法安全迁移：{e}。原工程保持不变。", layer.name),
                )
            })?;
        p.x_mm = f64::from(mapped.paint_uv.x);
        p.y_mm = f64::from(mapped.paint_uv.y);
        p.rotation_deg = f64::from(mapped.rotation_rad.to_degrees());
        p.flip_x ^= mapped.reflected;
        p.surface_id = mapped.part_id.clone();
        p.anchor = Some(itasha_contracts::SurfaceAnchor {
            part_id: mapped.part_id,
            region_id: mapped.region_id,
            source_triangle_id: mapped.triangle.stable_key(),
            barycentric: mapped.barycentric.to_array(),
            paint_surface_fingerprint: current.to_string(),
        });
        Ok(())
    })
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
            let mut candidates = vec![root.join("paint/paint-map.json")];
            candidates.extend(paint_candidates(
                manifest()["runtimePaint"]
                    .as_str()
                    .expect("runtime paint path"),
            ));
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
