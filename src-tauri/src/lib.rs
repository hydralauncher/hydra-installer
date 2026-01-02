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
async fn check_previous_installation() -> Result<bool, String> {
    let home_dir = dirs::home_dir()
        .ok_or("Failed to get home directory")?;
    
    let mut hydra_path = home_dir;
    hydra_path.push("AppData");
    hydra_path.push("Roaming");
    hydra_path.push("hydralauncher");
    
    Ok(hydra_path.exists() && hydra_path.is_dir())
}

#[tauri::command]
async fn kill_hydra_process() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use tokio::process::Command;
        
        let output = Command::new("taskkill")
            .args(&["/F", "/IM", "Hydra.exe", "/T"])
            .output()
            .await;
        
        match output {
            Ok(result) => {
                if result.status.success() || result.status.code() == Some(128) {
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if stderr.contains("not found") || stderr.contains("not running") {
                        Ok(())
                    } else {
                        Err(format!("Failed to kill Hydra process: {}", stderr))
                    }
                }
            }
            Err(e) => {
                Err(format!("Failed to execute taskkill: {}", e))
            }
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        Err("Killing Hydra process is only supported on Windows".to_string())
    }
}

#[tauri::command]
async fn delete_previous_installation() -> Result<(), String> {
    kill_hydra_process().await?;
    
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    let home_dir = dirs::home_dir()
        .ok_or("Failed to get home directory")?;
    
    let mut hydra_path = home_dir;
    hydra_path.push("AppData");
    hydra_path.push("Roaming");
    hydra_path.push("hydralauncher");
    
    if hydra_path.exists() && hydra_path.is_dir() {
        std::fs::remove_dir_all(&hydra_path)
            .map_err(|e| format!("Failed to delete previous installation: {}", e))?;
    }
    
    Ok(())
}

#[tauri::command]
async fn launch_hydra() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use tokio::process::Command;
        
        let home_dir = dirs::home_dir()
            .ok_or("Failed to get home directory")?;
        
        let mut hydra_path = home_dir;
        hydra_path.push("AppData");
        hydra_path.push("Local");
        hydra_path.push("Programs");
        hydra_path.push("Hydra");
        hydra_path.push("Hydra.exe");
        
        if !hydra_path.exists() {
            return Err(format!("Hydra executable not found at: {}", hydra_path.display()));
        }
        
        Command::new(&hydra_path)
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
    let download_dir = dirs::download_dir()
        .ok_or("Failed to get Downloads directory")?;
    
    let filename = url.split('/').last().unwrap_or("downloaded_file.exe");
    let file_path = download_dir.join(filename);
    
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
    use tokio::io::AsyncWriteExt;
    use std::time::Instant;
    
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
    
    if let Err(e) = window.emit("download-complete", &serde_json::json!({ 
        "path": file_path.to_string_lossy(),
        "total": total_size
    })) {
        return Err(format!("Failed to emit completion: {}", e));
    }
    
    #[cfg(target_os = "windows")]
    {
        use tokio::process::Command;
        
        let installer_path = file_path.to_string_lossy().to_string();
        let mut cmd = Command::new(installer_path);
        cmd.args(&["/S", "/NORESTART"]);
        
        match cmd.spawn() {
            Ok(mut child) => {
                let status = child.wait().await
                    .map_err(|e| format!("Failed to wait for installer: {}", e))?;
                
                if status.success() {
                    window.emit("install-complete", &serde_json::json!({ "success": true })).ok();
                } else {
                    let error_msg = format!("Installer exited with code: {:?}", status.code());
                    window.emit("install-error", &serde_json::json!({ "error": error_msg.clone() })).ok();
                    return Err(error_msg);
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to start installer: {}", e);
                window.emit("install-error", &serde_json::json!({ "error": error_msg.clone() })).ok();
                return Err(error_msg);
            }
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        return Err("Installation is only supported on Windows".to_string());
    }
    
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![start_download, show_main_window, exit_app, launch_hydra, check_previous_installation, delete_previous_installation])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
