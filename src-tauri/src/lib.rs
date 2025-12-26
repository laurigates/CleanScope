//! `CleanScope` - Privacy-respecting USB endoscope viewer
//!
//! This module contains the core Tauri application logic and USB camera handling.

mod usb;

#[cfg(target_os = "android")]
mod libusb_android;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

/// Shared frame buffer for storing the latest camera frame
pub struct FrameBuffer {
    /// Raw JPEG frame data
    pub frame: Vec<u8>,
    /// Timestamp when frame was captured
    pub timestamp: Instant,
    /// Frame width in pixels
    pub width: u32,
    /// Frame height in pixels
    pub height: u32,
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self {
            frame: Vec::new(),
            timestamp: Instant::now(),
            width: 0,
            height: 0,
        }
    }
}

/// Application state managed by Tauri
pub struct AppState {
    /// Shared frame buffer protected by mutex
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
}

/// USB device connection status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbStatus {
    /// Whether a USB device is currently connected
    pub connected: bool,
    /// Optional information about the connected device
    pub info: Option<String>,
}

/// Camera resolution information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
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

/// Get the latest camera frame as raw bytes
///
/// Returns the frame as an `ipc::Response` containing raw JPEG data,
/// which is transferred to JavaScript as an `ArrayBuffer` without Base64 encoding.
#[tauri::command]
fn get_frame(state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    let buffer = state
        .frame_buffer
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    if buffer.frame.is_empty() {
        return Err("No frame available".to_string());
    }

    Ok(tauri::ipc::Response::new(buffer.frame.clone()))
}

/// Emit a USB device event to the frontend
pub fn emit_usb_event(app: &AppHandle, connected: bool, info: Option<String>) {
    let _ = app.emit("usb-device-event", UsbStatus { connected, info });
}

/// Emit a camera frame event to the frontend
pub fn emit_camera_frame(app: &AppHandle, width: u32, height: u32) {
    let _ = app.emit("camera-frame", Resolution { width, height });
}

/// Run the `CleanScope` application
///
/// Initializes logging, sets up the Tauri builder with commands and plugins,
/// and starts the application.
///
/// # Panics
///
/// Panics if the Tauri application fails to start.
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

    // Create shared frame buffer for camera frames
    let frame_buffer = Arc::new(Mutex::new(FrameBuffer::default()));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            frame_buffer: Arc::clone(&frame_buffer),
        })
        .invoke_handler(tauri::generate_handler![
            check_usb_status,
            cycle_resolution,
            get_resolutions,
            get_frame,
        ])
        .setup(move |_app| {
            log::info!("Tauri app setup complete");

            // On Android, we'll initialize the USB handling here
            #[cfg(target_os = "android")]
            {
                let app_handle = _app.handle().clone();
                let frame_buffer_clone = Arc::clone(&frame_buffer);
                std::thread::spawn(move || {
                    usb::init_usb_handler(app_handle, frame_buffer_clone);
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
