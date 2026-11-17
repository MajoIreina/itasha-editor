fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "read_view",
            "execute",
            "close_clean",
            "choose_directory",
            "viewport_load",
            "viewport_bounds",
            "viewport_status",
            "viewport_reset",
            "viewport_focus",
            "viewport_select",
            "viewport_pointer",
            "viewport_probe",
            "import_image",
            "import_image_path",
            "asset_bytes",
            "viewport_surface",
            "viewport_coverage",
            "viewport_transport",
            "viewport_gesture",
            "viewport_decal",
        ]),
    ))
    .expect("Tauri build failed");
}
