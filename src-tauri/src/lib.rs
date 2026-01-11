use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Window};

#[derive(Serialize, Deserialize)]
struct DownloadProgress {
    downloaded: u64,
    total: Option<u64>,
    percentage: f64,
    speed: f64,
    eta: Option<f64>,
}

#[tauri::command]
async fn show_main_window(window: Window) {
    window.get_webview_window("main").unwrap().show().unwrap();
}

#[tauri::command]
async fn exit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn minimize_window(window: Window) {
    if let Some(webview_window) = window.get_webview_window("main") {
        let _ = webview_window.minimize();
    }
}

#[tauri::command]
async fn check_previous_installation() -> Result<bool, String> {
    let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;

    let mut hydra_path = home_dir;
    hydra_path.push("AppData");
    hydra_path.push("Roaming");
    hydra_path.push("hydralauncher");

    Ok(hydra_path.exists() && hydra_path.is_dir())
}

#[tauri::command]
async fn get_hydra_installation_path() -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        let registry_paths = vec![
            (
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                HKEY_LOCAL_MACHINE,
            ),
            (
                "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                HKEY_LOCAL_MACHINE,
            ),
            (
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                HKEY_CURRENT_USER,
            ),
        ];

        for (path, hkey) in registry_paths {
            let hkcu = RegKey::predef(hkey);
            let uninstall_key = match hkcu.open_subkey(path) {
                Ok(key) => key,
                Err(_) => continue,
            };

            for key_name in uninstall_key.enum_keys().map(|x| x.unwrap()) {
                let subkey = match uninstall_key.open_subkey(&key_name) {
                    Ok(key) => key,
                    Err(_) => continue,
                };

                let display_name: String = match subkey.get_value("DisplayName") {
                    Ok(name) => name,
                    Err(_) => continue,
                };

                let publisher: String = match subkey.get_value("Publisher") {
                    Ok(pub_name) => pub_name,
                    Err(_) => continue,
                };

                if display_name == "Hydra" && publisher == "Los Broxas" {
                    if let Ok(install_location) = subkey.get_value::<String, _>("InstallLocation") {
                        if !install_location.is_empty() {
                            return Ok(Some(install_location));
                        }
                    }

                    if let Ok(uninstall_string) = subkey.get_value::<String, _>("UninstallString") {
                        let uninstall_path = {
                            let trimmed = uninstall_string.trim();
                            if trimmed.starts_with('"') {
                                if let Some(end_quote) = trimmed[1..].find('"') {
                                    trimmed[1..end_quote + 1].to_string()
                                } else {
                                    trimmed
                                        .trim_matches('"')
                                        .split_whitespace()
                                        .next()
                                        .unwrap_or(trimmed)
                                        .to_string()
                                }
                            } else {
                                trimmed
                                    .split_whitespace()
                                    .next()
                                    .unwrap_or(trimmed)
                                    .to_string()
                            }
                        };

                        if let Some(parent) = std::path::Path::new(&uninstall_path).parent() {
                            let hydra_exe = parent.join("Hydra.exe");
                            return Ok(Some(hydra_exe.to_string_lossy().to_string()));
                        }
                    }

                    if let Ok(display_icon) = subkey.get_value::<String, _>("DisplayIcon") {
                        let icon_path = display_icon.split(',').next().unwrap_or(&display_icon);
                        if let Some(parent) = std::path::Path::new(icon_path).parent() {
                            return Ok(Some(parent.to_string_lossy().to_string()));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Registry lookup is only supported on Windows".to_string())
    }
}

#[tauri::command]
async fn delete_previous_installation() -> Result<(), String> {
    let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;

    let mut hydra_path = home_dir;
    hydra_path.push("AppData");
    hydra_path.push("Roaming");
    hydra_path.push("hydralauncher");

    if hydra_path.exists() && hydra_path.is_dir() {
        trash::delete(&hydra_path)
            .map_err(|e| format!("Failed to delete previous installation: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
async fn launch_hydra() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        let hydra_path_str = get_hydra_installation_path()
            .await?
            .ok_or("Hydra installation not found in registry")?;

        let hydra_path = std::path::Path::new(&hydra_path_str);

        if !hydra_path.exists() {
            return Err(format!(
                "Hydra executable not found at: {}",
                hydra_path.display()
            ));
        }

        Command::new("cmd")
            .args(&["/C", "start", "", &hydra_path_str])
            .spawn()
            .map_err(|e| format!("Failed to launch Hydra: {}", e))?;

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Launching Hydra is only supported on Windows".to_string())
    }
}

#[tauri::command]
async fn start_download(window: Window, url: String) -> Result<(), String> {
    let temp_dir = std::env::temp_dir();

    let filename = url.split('/').last().unwrap_or("downloaded_file.exe");
    let file_path = temp_dir.join(filename);

    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    let total_size = response.content_length();
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&file_path)
        .await
        .map_err(|e| format!("Failed to create file: {}", e))?;

    use futures_util::StreamExt;
    use std::time::Instant;
    use tokio::io::AsyncWriteExt;

    let start_time = Instant::now();
    let mut last_update_time = start_time;

    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(chunk) => chunk,
            Err(e) => {
                let error_msg = format!("Download error: {}", e);
                let _ = window.emit("download-error", &serde_json::json!({ "error": error_msg }));
                return Err(error_msg);
            }
        };

        if let Err(e) = file.write_all(&chunk).await {
            let error_msg = format!("Write error: {}", e);
            let _ = window.emit("download-error", &serde_json::json!({ "error": error_msg }));
            return Err(error_msg);
        }

        downloaded += chunk.len() as u64;

        let current_time = Instant::now();
        let elapsed = current_time.duration_since(last_update_time).as_secs_f64();

        if elapsed >= 0.1 {
            let total_elapsed = current_time.duration_since(start_time).as_secs_f64();
            let speed = if total_elapsed > 0.0 {
                downloaded as f64 / total_elapsed
            } else {
                0.0
            };

            let percentage = if let Some(total) = total_size {
                (downloaded as f64 / total as f64) * 100.0
            } else {
                -1.0
            };

            let eta = if let Some(total) = total_size {
                if speed > 0.0 && downloaded < total {
                    Some((total - downloaded) as f64 / speed)
                } else {
                    None
                }
            } else {
                None
            };

            let progress = DownloadProgress {
                downloaded,
                total: total_size,
                percentage,
                speed,
                eta,
            };

            if let Err(e) = window.emit("download-progress", &progress) {
                return Err(format!("Failed to emit progress: {}", e));
            }

            last_update_time = current_time;
        }
    }

    drop(file);

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    if let Err(e) = window.emit(
        "download-complete",
        &serde_json::json!({
            "path": file_path.to_string_lossy(),
            "total": total_size
        }),
    ) {
        return Err(format!("Failed to emit completion: {}", e));
    }

    Ok(())
}

#[tauri::command]
async fn run_installer(installer_path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/C", "start", "", &installer_path])
            .spawn()
            .map_err(|e| format!("Failed to run installer: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        return Err("Running installer is only supported on Windows".to_string());
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            start_download,
            show_main_window,
            exit_app,
            minimize_window,
            launch_hydra,
            check_previous_installation,
            delete_previous_installation,
            get_hydra_installation_path,
            run_installer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
