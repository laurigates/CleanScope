//! CleanScope - Privacy-respecting USB endoscope viewer
//!
//! This module contains the core Tauri application logic and USB camera handling.

mod usb;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// USB device connection status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbStatus {
    pub connected: bool,
    pub info: Option<String>,
}

/// Camera resolution information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Check the current USB device status
#[tauri::command]
fn check_usb_status() -> Result<UsbStatus, String> {
    // TODO: Implement actual USB status check via JNI on Android
    log::info!("Checking USB status");
    Ok(UsbStatus {
        connected: false,
        info: None,
    })
}

/// Cycle through available camera resolutions
#[tauri::command]
fn cycle_resolution() -> Result<String, String> {
    // TODO: Implement resolution cycling
    log::info!("Cycling resolution");
    Ok("640x480".to_string())
}

/// Get the list of available resolutions
#[tauri::command]
fn get_resolutions() -> Result<Vec<Resolution>, String> {
    // TODO: Query camera for supported resolutions
    Ok(vec![
        Resolution {
            width: 640,
            height: 480,
        },
        Resolution {
            width: 1280,
            height: 720,
        },
    ])
}

/// Emit a USB device event to the frontend
pub fn emit_usb_event(app: &AppHandle, connected: bool, info: Option<String>) {
    let _ = app.emit("usb-device-event", UsbStatus { connected, info });
}

/// Emit a camera frame event to the frontend
pub fn emit_camera_frame(app: &AppHandle, width: u32, height: u32) {
    let _ = app.emit("camera-frame", Resolution { width, height });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    #[cfg(target_os = "android")]
    {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Debug)
                .with_tag("CleanScope"),
        );
    }

    #[cfg(not(target_os = "android"))]
    {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }

    log::info!("CleanScope starting up");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            check_usb_status,
            cycle_resolution,
            get_resolutions,
        ])
        .setup(|_app| {
            log::info!("Tauri app setup complete");

            // On Android, we'll initialize the USB handling here
            #[cfg(target_os = "android")]
            {
                let app_handle = _app.handle().clone();
                std::thread::spawn(move || {
                    usb::init_usb_handler(app_handle);
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
