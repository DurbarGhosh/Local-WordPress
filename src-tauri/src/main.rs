// Prevents an extra console window on Windows release builds; harmless on Mac.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Manager, RunEvent, State};
use wplocal_core::{Overview, Runner};

#[tauri::command]
async fn list_sites(runner: State<'_, Runner>) -> Result<Overview, String> {
    Ok(runner.overview())
}

#[tauri::command]
async fn start_site(runner: State<'_, Runner>, id: String) -> Result<(), String> {
    runner.start_site(&id)
}

#[tauri::command]
async fn stop_site(runner: State<'_, Runner>, id: String) -> Result<(), String> {
    runner.stop_site(&id)
}

/// Opens a web address in the default browser.
#[tauri::command]
async fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("Only web addresses can be opened".into());
    }
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(Runner::new())
        .invoke_handler(tauri::generate_handler![list_sites, start_site, stop_site, open_url])
        .build(tauri::generate_context!())
        .expect("error while building the app")
        .run(|app, event| {
            // Quitting the app stops every site it started, so nothing is left running.
            if let RunEvent::Exit = event {
                app.state::<Runner>().stop_all();
            }
        });
}
