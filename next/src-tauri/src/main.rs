#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adapters;
mod commands;
mod host;
mod platform;

fn main() {
    host::builder()
        .invoke_handler(tauri::generate_handler![
            commands::editor::read_view,
            commands::editor::execute,
            commands::editor::close_clean,
            commands::assets::choose_directory,
            commands::viewport::viewport_load,
            commands::viewport::viewport_bounds,
            commands::viewport::viewport_status,
            commands::viewport::viewport_reset,
            commands::viewport::viewport_focus,
            commands::viewport::viewport_select,
            commands::viewport::viewport_pointer,
            commands::viewport::viewport_gesture,
            commands::viewport::viewport_probe,
            commands::assets::import_image,
            commands::assets::import_image_path,
            commands::assets::asset_bytes,
            commands::viewport::viewport_surface,
            commands::viewport::viewport_coverage,
            commands::viewport::viewport_transport,
            commands::viewport::viewport_decal
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start desktop shell");
}
