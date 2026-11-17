use itasha_surface_map::PaintSurfaceMap;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub fn paint_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/paint")
}

pub fn runtime_map() -> &'static PaintSurfaceMap {
    static MAP: OnceLock<PaintSurfaceMap> = OnceLock::new();
    MAP.get_or_init(|| {
        let explicit = std::env::var_os("ITASHA_TEST_PAINT_MAP").map(PathBuf::from);
        let (path, manifest) = if let Some(path) = explicit {
            let path = if path.is_absolute() {
                path
            } else {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(path)
            };
            (path, None)
        } else {
            let manifest: Value = serde_json::from_slice(
                &std::fs::read(paint_root().join("manifest.json")).expect("paint manifest"),
            )
            .expect("valid paint manifest");
            let path = paint_root().join(
                manifest["runtimePaint"]
                    .as_str()
                    .expect("runtime paint path"),
            );
            (path, Some(manifest))
        };
        let map: PaintSurfaceMap = serde_json::from_slice(
            &std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())),
        )
        .expect("valid runtime paint map");
        map.validate().expect("sealed runtime map");
        if let Some(manifest) = manifest {
            assert_eq!(
                Some(map.paint_fingerprint.as_str()),
                manifest["paintFingerprint"].as_str()
            );
            assert_eq!(
                Some(map.model_fingerprint.as_str()),
                manifest["modelFingerprint"].as_str()
            );
        }
        map
    })
}
