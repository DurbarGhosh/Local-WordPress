// Prevents an extra console window on Windows release builds; harmless on Mac.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tauri::command]
fn list_sites() -> wplocal_core::Overview {
    wplocal_core::list_sites()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![list_sites])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
