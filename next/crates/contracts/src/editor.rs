use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorDocument {
    pub model_fingerprint: Option<String>,
    pub layers: Vec<Layer>,
    pub assets: Vec<ImageAsset>,
}
impl EditorDocument {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn validate(&self, vehicle: Option<&str>) -> AppResult<()> {
        let bad = || AppError::new("invalid-layer", "图层、部件映射或素材数据无效。");
        if self.layers.len() > 200 || self.assets.len() > 128 {
            return Err(bad());
        }
        if self
            .assets
            .iter()
            .map(|a| u64::from(a.width) * u64::from(a.height))
            .sum::<u64>()
            > 67_108_864
        {
            return Err(AppError::new(
                "asset-budget",
                "素材解码总像素超出 256 MiB 预算。",
            ));
        }
        if self
            .model_fingerprint
            .as_ref()
            .is_some_and(|s| !valid_hash(s))
            || (!self.layers.is_empty() && self.model_fingerprint.is_none())
        {
            return Err(bad());
        }
        let mut ids = std::collections::HashSet::new();
        for asset in &self.assets {
            if !ids.insert(asset.id.clone())
                || !valid_hash(&asset.id)
                || asset.width == 0
                || asset.height == 0
                || asset.width > 8192
                || asset.height > 8192
                || u64::from(asset.width) * u64::from(asset.height) > 16_777_216
                || asset.name.is_empty()
                || asset.name.len() > 300
            {
                return Err(bad());
            }
        }
        let mut ids = std::collections::HashSet::new();
        for layer in &self.layers {
            if !ids.insert(layer.id) {
                return Err(bad());
            }
            layer.validate(vehicle)?;
            if layer.placement.anchor.as_ref().is_none_or(|a| {
                Some(&a.paint_surface_fingerprint) != self.model_fingerprint.as_ref()
            }) {
                return Err(bad());
            }
            if let LayerSource::Image { asset_id } = &layer.source
                && !self.assets.iter().any(|a| &a.id == asset_id)
            {
                return Err(bad());
            }
        }
        Ok(())
    }
}
pub fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageAsset {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceAnchor {
    pub part_id: String,
    pub region_id: String,
    /// Stable string preserves all 64 fragment bits across the JavaScript boundary.
    pub source_triangle_id: String,
    pub barycentric: [f32; 3],
    pub paint_surface_fingerprint: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Placement {
    pub anchor: Option<SurfaceAnchor>,
    pub surface_id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cross_surface: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip_x: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aspect_ratio_locked: bool,
    pub profile_version: u32,
    pub x_mm: f64,
    pub y_mm: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    pub rotation_deg: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f64,
    pub placement: Placement,
    pub source: LayerSource,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LayerSource {
    Shape {
        shape: Shape,
        fill: String,
    },
    Text {
        text: String,
        font_family: String,
        fill: String,
    },
    Image {
        asset_id: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    Rectangle,
    Ellipse,
    Triangle,
    Line,
    Star,
    Hexagon,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EditOperation {
    Insert { layer: Layer, fingerprint: String },
    Update { layer: Layer },
    Delete { id: Uuid },
    Reorder { id: Uuid, offset: i32 },
}
impl Layer {
    pub fn validate(&self, vehicle: Option<&str>) -> AppResult<()> {
        let bad = || {
            AppError::new(
                "invalid-layer",
                "图层参数超出范围，或目标不是已注册的车漆外板。",
            )
        };
        let p = &self.placement;
        if self.name.trim().is_empty()
            || self.name.chars().count() > 100
            || self.name.chars().any(char::is_control)
            || vehicle != Some("su7")
            || !SU7_SURFACES.iter().any(|s| s.id == p.surface_id)
            || p.profile_version != 2
            || p.anchor.as_ref().is_none_or(|a| {
                a.part_id != p.surface_id
                    || a.region_id.is_empty()
                    || a.source_triangle_id.is_empty()
                    || !valid_hash(&a.paint_surface_fingerprint)
                    || a.barycentric
                        .iter()
                        .any(|v| !v.is_finite() || *v < -0.0001 || *v > 1.0001)
                    || (a.barycentric.iter().sum::<f32>() - 1.).abs() > 0.0001
            })
            || ![
                p.x_mm,
                p.y_mm,
                p.width_mm,
                p.height_mm,
                p.rotation_deg,
                self.opacity,
            ]
            .iter()
            .all(|v| v.is_finite())
            || p.x_mm.abs() > 20_000.
            || p.y_mm.abs() > 20_000.
            || !(1.0..=10_000.).contains(&p.width_mm)
            || !(1.0..=10_000.).contains(&p.height_mm)
            || p.rotation_deg.abs() > 360.
            || !(0.0..=1.).contains(&self.opacity)
        {
            return Err(bad());
        }
        let color = |s: &str| {
            s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|v| v.is_ascii_hexdigit())
        };
        match &self.source {
            LayerSource::Shape { fill, .. } if !color(fill) => return Err(bad()),
            LayerSource::Text {
                text,
                font_family,
                fill,
            } => {
                if text.trim().is_empty()
                    || text.chars().count() > 1000
                    || !color(fill)
                    || !["Segoe UI", "Microsoft YaHei", "Arial", "Consolas"]
                        .contains(&font_family.as_str())
                {
                    return Err(bad());
                }
            }
            LayerSource::Image { asset_id } if !valid_hash(asset_id) => return Err(bad()),
            _ => {}
        }
        Ok(())
    }
}
/// Versioned SU7 adapter, separated from generic projection and rendering.
/// Body paint-switch alone is insufficient: wheel inserts and inner panels also use it.
pub struct SurfaceSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub mesh_id: &'static str,
    pub node_name: &'static str,
    pub region_bounds: Option<[[f32; 3]; 2]>,
    pub whole_components: bool,
    pub u: [f32; 3],
    pub v: [f32; 3],
    pub normal: [f32; 3],
}
pub const SU7_SURFACES: &[SurfaceSpec] = &[
    SurfaceSpec {
        id: "su7.front-left-door",
        name: "左前门外板",
        mesh_id: "body.glb/node/44/primitive/0",
        node_name: "carPaint_external_4",
        region_bounds: None,
        whole_components: false,
        u: [1., 0., 0.],
        v: [0., -1., 0.],
        normal: [0., 0., 1.],
    },
    SurfaceSpec {
        id: "su7.rear-left-door",
        name: "左后门外板",
        mesh_id: "body.glb/node/26/primitive/0",
        node_name: "carPaint_external_5",
        region_bounds: None,
        whole_components: false,
        u: [1., 0., 0.],
        v: [0., -1., 0.],
        normal: [0., 0., 1.],
    },
    SurfaceSpec {
        id: "su7.front-right-door",
        name: "右前门外板",
        mesh_id: "body.glb/node/78/primitive/0",
        node_name: "carPaint_external_6",
        region_bounds: None,
        whole_components: false,
        u: [-1., 0., 0.],
        v: [0., -1., 0.],
        normal: [0., 0., -1.],
    },
    SurfaceSpec {
        id: "su7.rear-right-door",
        name: "右后门外板",
        mesh_id: "body.glb/node/60/primitive/0",
        node_name: "carPaint_external_7",
        region_bounds: None,
        whole_components: false,
        u: [-1., 0., 0.],
        v: [0., -1., 0.],
        normal: [0., 0., -1.],
    },
    SurfaceSpec {
        id: "su7.hood",
        name: "前机盖外板",
        mesh_id: "body.glb/node/13/primitive/0",
        node_name: "carPaint_external_3_1",
        region_bounds: None,
        whole_components: false,
        u: [0., 0., 1.],
        v: [1., 0., 0.],
        normal: [0., 1., 0.],
    },
    SurfaceSpec {
        id: "su7.trunk",
        name: "尾厢盖外板",
        mesh_id: "body.glb/node/91/primitive/0",
        node_name: "carPaint_external_8",
        region_bounds: None,
        whole_components: false,
        u: [0., 0., -1.],
        v: [1., 0., 0.],
        normal: [0., 1., 0.],
    },
    SurfaceSpec {
        id: "su7.front-bumper",
        name: "前保险杠外板",
        mesh_id: "body.glb/node/174/primitive/0",
        node_name: "carPaint_external_10",
        region_bounds: None,
        whole_components: false,
        u: [0., 0., 1.],
        v: [0., -1., 0.],
        normal: [-1., 0., 0.],
    },
    SurfaceSpec {
        id: "su7.front-c-ring",
        name: "前 C 环油漆面",
        mesh_id: "body.glb/node/199/primitive/0",
        node_name: "carTrimCRing_Front",
        region_bounds: None,
        whole_components: false,
        u: [0., 0., 1.],
        v: [0., -1., 0.],
        normal: [-1., 0., 0.],
    },
    SurfaceSpec {
        id: "su7.rear-c-ring",
        name: "后 C 环油漆面",
        mesh_id: "body.glb/node/198/primitive/0",
        node_name: "carTrimCRing_Back",
        region_bounds: None,
        whole_components: false,
        u: [0., 0., -1.],
        v: [0., -1., 0.],
        normal: [1., 0., 0.],
    },
    SurfaceSpec {
        id: "su7.rear-left-fender",
        name: "左后翼子板及立柱",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[0.60, 0.80, 0.70], [0.70, 0.90, 0.80]]),
        whole_components: true,
        u: [1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    },
    SurfaceSpec {
        id: "su7.rear-right-fender",
        name: "右后翼子板及立柱",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[0.60, 0.80, -0.80], [0.70, 0.90, -0.70]]),
        whole_components: true,
        u: [-1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, -1.0],
    },
    SurfaceSpec {
        id: "su7.charge-cover",
        name: "充电盖",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        // Anchor the disconnected charging-cover component on the left rear quarter.
        region_bounds: Some([[1.82, 0.90, 0.83], [1.85, 0.94, 0.87]]),
        whole_components: true,
        u: [0., 1., 0.],
        v: [1., 0., 0.],
        normal: [0., 0., 1.],
    },
    SurfaceSpec {
        id: "su7.rear-bumper",
        name: "后保险杠",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[1.6, -9.0, -9.0], [9.0, 0.83, 9.0]]),
        whole_components: false,
        u: [0.0, 0.0, -1.0],
        v: [0.0, -1.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    },
    SurfaceSpec {
        id: "su7.front-left-lower-fender",
        name: "左前下翼子板",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[-1.38, 0.24, 0.91], [-0.90, 0.76, 0.99]]),
        whole_components: true,
        u: [1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    },
    SurfaceSpec {
        id: "su7.front-right-lower-fender",
        name: "右前下翼子板",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[-1.38, 0.24, -0.99], [-0.90, 0.76, -0.91]]),
        whole_components: true,
        u: [-1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, -1.0],
    },
    SurfaceSpec {
        id: "su7.front-left-fender",
        name: "左前上翼子板",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[-9.0, -9.0, 0.0], [-0.88, 9.0, 9.0]]),
        whole_components: false,
        u: [1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    },
    SurfaceSpec {
        id: "su7.front-right-fender",
        name: "右前上翼子板",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[-9.0, -9.0, -9.0], [-0.88, 9.0, 0.0]]),
        whole_components: false,
        u: [-1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, -1.0],
    },
    SurfaceSpec {
        id: "su7.left-sill",
        name: "左侧裙",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[-9.0, -9.0, 0.0], [9.0, 0.4, 9.0]]),
        whole_components: false,
        u: [1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    },
    SurfaceSpec {
        id: "su7.right-sill",
        name: "右侧裙",
        mesh_id: "body.glb/node/175/primitive/0",
        node_name: "carPaint_internal_1",
        region_bounds: Some([[-9.0, -9.0, -9.0], [9.0, 0.4, 0.0]]),
        whole_components: false,
        u: [-1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, -1.0],
    },
    SurfaceSpec {
        id: "su7.rear-spoiler",
        name: "尾翼油漆面",
        mesh_id: "body.glb/node/140/primitive/0",
        node_name: "carPaint_external_2",
        region_bounds: None,
        whole_components: false,
        u: [0.0, 0.0, -1.0],
        v: [1.0, 0.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    },
    SurfaceSpec {
        id: "su7.rear-trim",
        name: "尾部牌照与尾厢收边",
        mesh_id: "body.glb/node/180/primitive/0",
        node_name: "carPaint_internal_23",
        region_bounds: None,
        whole_components: false,
        u: [0.0, 0.0, -1.0],
        v: [0.0, -1.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    },
    SurfaceSpec {
        id: "su7.cowl",
        name: "前机盖后缘油漆面",
        mesh_id: "body.glb/node/16/primitive/0",
        node_name: "carPaint_internal_22",
        region_bounds: None,
        whole_components: false,
        u: [0.0, 0.0, 1.0],
        v: [1.0, 0.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceInfo {
    pub id: String,
    pub name: String,
    pub profile_version: u32,
    pub width_mm: f64,
    pub height_mm: f64,
    pub triangles: usize,
    pub projection: [[f32; 4]; 3],
    pub depth: f32,
}
