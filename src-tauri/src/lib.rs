//! `CleanScope` - Privacy-respecting USB endoscope viewer
//!
//! This module contains the core Tauri application logic and USB camera handling.

mod usb;

#[cfg(target_os = "android")]
mod libusb_android;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

/// Shared frame buffer for storing the latest camera frame
pub struct FrameBuffer {
    /// Processed frame data (JPEG or RGB)
    pub frame: Vec<u8>,
    /// Raw frame data before conversion (for debugging)
    pub raw_frame: Vec<u8>,
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
            raw_frame: Vec::new(),
            timestamp: Instant::now(),
            width: 0,
            height: 0,
        }
    }
}

/// Display settings that can be adjusted independently
#[derive(Debug, Clone, Copy, Default)]
pub struct DisplaySettings {
    /// Width override (None = auto-detect from frame size)
    pub width: Option<u32>,
    /// Height override (None = auto-detect from frame size)
    pub height: Option<u32>,
    /// Stride override in bytes (None = width * 2 for YUY2)
    pub stride: Option<u32>,
    /// Row offset - skip this many bytes at start of frame
    pub row_offset: i32,
}

/// Available width options for cycling
pub const WIDTH_OPTIONS: &[u32] = &[1280, 1920, 640, 800, 1024, 960, 720, 1440];

/// Available height options for cycling
pub const HEIGHT_OPTIONS: &[u32] = &[720, 1080, 480, 600, 768, 540, 960, 800];

/// Available stride options (as multiplier of width, e.g., 2.0 = width*2)
/// Added more fine-grained options for cheap cameras with non-standard padding
pub const STRIDE_OPTIONS: &[f32] = &[
    2.0,   // Standard YUY2: width * 2
    2.004, // ~5 extra bytes per row for 1280 width (1280*2.004 = 2565)
    2.008, // ~10 extra bytes per row
    2.016, // ~20 extra bytes per row
    2.032, // ~40 extra bytes per row (64-byte alignment padding)
    2.5,   // 25% extra
    3.0,   // 50% extra
];

/// Available row offset options (bytes to skip at start of frame)
/// These help find the correct frame alignment for cameras with embedded headers
pub const OFFSET_OPTIONS: &[i32] = &[
    0, 2, 4, 8, 12, 16, 32, 64, 128, 256, 512, // Skip bytes at start
    -2, -4, -8, // Negative values (for testing)
];

/// Application state managed by Tauri
pub struct AppState {
    /// Shared frame buffer protected by mutex
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
    /// Display settings for resolution/stride overrides
    pub display_settings: Arc<Mutex<DisplaySettings>>,
    /// Current width option index (None = auto)
    pub width_index: Arc<Mutex<Option<usize>>>,
    /// Current height option index (None = auto)
    pub height_index: Arc<Mutex<Option<usize>>>,
    /// Current stride option index (None = auto)
    pub stride_index: Arc<Mutex<Option<usize>>>,
    /// Current offset option index
    pub offset_index: Arc<Mutex<usize>>,
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

/// Build information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    /// Application version from Cargo.toml
    pub version: String,
    /// Git commit hash (short)
    pub git_hash: String,
    /// Build timestamp
    pub build_time: String,
}

/// Get build information (version, git hash, build time)
#[tauri::command]
fn get_build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_hash: env!("BUILD_GIT_HASH").to_string(),
        build_time: env!("BUILD_TIMESTAMP").to_string(),
    }
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

/// Frame information returned to frontend
#[derive(Debug, Clone, serde::Serialize)]
struct FrameInfo {
    width: u32,
    height: u32,
    /// "jpeg" or "rgb"
    format: String,
}

/// Get the latest camera frame as raw bytes
///
/// Returns the frame as an `ipc::Response` containing raw pixel data,
/// which is transferred to JavaScript as an `ArrayBuffer` without Base64 encoding.
/// The data format depends on the camera:
/// - MJPEG cameras: JPEG-encoded data
/// - YUY2 cameras: Raw RGB24 data (3 bytes per pixel)
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

/// Captured frame information returned to frontend
#[derive(Debug, Clone, serde::Serialize)]
struct CapturedFrame {
    /// Path where processed frame was saved
    path: String,
    /// Path where raw frame was saved (if available)
    raw_path: Option<String>,
    /// Processed frame size in bytes
    size: usize,
    /// Raw frame size in bytes
    raw_size: usize,
    /// First 64 bytes of raw frame as hex for quick inspection
    header_hex: String,
    /// Detected format hint
    format_hint: String,
    /// Frame dimensions if known
    width: u32,
    height: u32,
}

/// Dump the current frame to files for analysis
///
/// Saves both the processed frame (RGB/JPEG) and the raw frame (YUY2) if available.
/// Returns information about the captured frames including file paths.
#[tauri::command]
fn dump_frame(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<CapturedFrame, String> {
    use std::io::Write;

    let buffer = state
        .frame_buffer
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    if buffer.frame.is_empty() {
        return Err("No frame available to dump".to_string());
    }

    // Get app cache directory (works on Android)
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Could not get cache dir: {}", e))?;

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Could not create cache dir: {}", e))?;

    // Generate filename with timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Detect format from raw frame first bytes (if available), otherwise from processed frame
    let raw_available = !buffer.raw_frame.is_empty();
    let analysis_data = if raw_available {
        &buffer.raw_frame
    } else {
        &buffer.frame
    };

    let (format_hint, raw_extension) =
        if analysis_data.len() >= 2 && analysis_data[0] == 0xFF && analysis_data[1] == 0xD8 {
            ("MJPEG (JPEG SOI marker)", "jpg")
        } else {
            // Check if it looks like YUY2 (alternating Y/U/Y/V pattern)
            // YUY2 typically has Y values (0-255) with U/V around 128 for gray
            let possible_yuy2 = analysis_data.len() >= 4
                && analysis_data
                    .iter()
                    .skip(1)
                    .step_by(2)
                    .take(100)
                    .all(|&b| b > 100 && b < 156);
            if possible_yuy2 {
                ("YUY2/YUYV (UV values near 128)", "yuy2")
            } else {
                ("Raw video data", "raw")
            }
        };

    // Save processed frame (RGB or JPEG)
    let processed_ext =
        if buffer.frame.len() >= 2 && buffer.frame[0] == 0xFF && buffer.frame[1] == 0xD8 {
            "jpg"
        } else {
            "rgb"
        };
    let processed_filename = format!(
        "frame_{}_{}x{}.{}",
        timestamp, buffer.width, buffer.height, processed_ext
    );
    let processed_filepath = cache_dir.join(&processed_filename);

    let mut file = std::fs::File::create(&processed_filepath)
        .map_err(|e| format!("Could not create file: {}", e))?;
    file.write_all(&buffer.frame)
        .map_err(|e| format!("Could not write frame: {}", e))?;

    log::info!(
        "Dumped processed frame to {}: {} bytes",
        processed_filepath.display(),
        buffer.frame.len()
    );

    // Save raw frame if available
    let raw_path = if raw_available {
        let raw_filename = format!(
            "frame_{}_{}x{}_raw.{}",
            timestamp, buffer.width, buffer.height, raw_extension
        );
        let raw_filepath = cache_dir.join(&raw_filename);

        let mut file = std::fs::File::create(&raw_filepath)
            .map_err(|e| format!("Could not create raw file: {}", e))?;
        file.write_all(&buffer.raw_frame)
            .map_err(|e| format!("Could not write raw frame: {}", e))?;

        log::info!(
            "Dumped raw frame to {}: {} bytes, format: {}",
            raw_filepath.display(),
            buffer.raw_frame.len(),
            format_hint
        );

        Some(raw_filepath.to_string_lossy().to_string())
    } else {
        log::info!("No raw frame available (might be MJPEG mode)");
        None
    };

    // Generate hex dump of first 64 bytes of raw frame
    let header_bytes: Vec<u8> = analysis_data.iter().take(64).copied().collect();
    let header_hex = header_bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ");

    log::info!("Header: {}", header_hex);

    Ok(CapturedFrame {
        path: processed_filepath.to_string_lossy().to_string(),
        raw_path,
        size: buffer.frame.len(),
        raw_size: buffer.raw_frame.len(),
        header_hex,
        format_hint: format_hint.to_string(),
        width: buffer.width,
        height: buffer.height,
    })
}

/// Get frame metadata (dimensions and format)
#[tauri::command]
fn get_frame_info(state: State<'_, AppState>) -> Result<FrameInfo, String> {
    let buffer = state
        .frame_buffer
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    if buffer.frame.is_empty() {
        return Err("No frame available".to_string());
    }

    // Detect format based on JPEG signature
    let format = if buffer.frame.len() >= 2 && buffer.frame[0] == 0xFF && buffer.frame[1] == 0xD8 {
        "jpeg".to_string()
    } else {
        "rgb".to_string()
    };

    Ok(FrameInfo {
        width: buffer.width,
        height: buffer.height,
        format,
    })
}

/// Cycle through width options
#[tauri::command]
fn cycle_width(state: State<'_, AppState>) -> String {
    let mut index = state.width_index.lock().unwrap();
    let mut settings = state.display_settings.lock().unwrap();

    // Cycle: None -> 0 -> 1 -> ... -> N-1 -> None
    let new_index = match *index {
        None => Some(0),
        Some(i) if i + 1 < WIDTH_OPTIONS.len() => Some(i + 1),
        Some(_) => None,
    };

    *index = new_index;
    settings.width = new_index.map(|i| WIDTH_OPTIONS[i]);

    match new_index {
        None => "W:Auto".to_string(),
        Some(i) => format!("W:{}", WIDTH_OPTIONS[i]),
    }
}

/// Cycle through height options
#[tauri::command]
fn cycle_height(state: State<'_, AppState>) -> String {
    let mut index = state.height_index.lock().unwrap();
    let mut settings = state.display_settings.lock().unwrap();

    let new_index = match *index {
        None => Some(0),
        Some(i) if i + 1 < HEIGHT_OPTIONS.len() => Some(i + 1),
        Some(_) => None,
    };

    *index = new_index;
    settings.height = new_index.map(|i| HEIGHT_OPTIONS[i]);

    match new_index {
        None => "H:Auto".to_string(),
        Some(i) => format!("H:{}", HEIGHT_OPTIONS[i]),
    }
}

/// Cycle through stride options
#[tauri::command]
fn cycle_stride(state: State<'_, AppState>) -> String {
    let mut index = state.stride_index.lock().unwrap();

    let new_index = match *index {
        None => Some(0),
        Some(i) if i + 1 < STRIDE_OPTIONS.len() => Some(i + 1),
        Some(_) => None,
    };

    *index = new_index;

    match new_index {
        None => "S:Auto".to_string(),
        Some(i) => format!("S:x{:.3}", STRIDE_OPTIONS[i]),
    }
}

/// Cycle through row offset options
#[tauri::command]
fn cycle_offset(state: State<'_, AppState>) -> String {
    let mut index = state.offset_index.lock().unwrap();
    let mut settings = state.display_settings.lock().unwrap();

    let new_index = (*index + 1) % OFFSET_OPTIONS.len();
    *index = new_index;
    settings.row_offset = OFFSET_OPTIONS[new_index];

    format!("O:{:+}", OFFSET_OPTIONS[new_index])
}

/// Get current display settings as a summary string
#[tauri::command]
fn get_display_settings(state: State<'_, AppState>) -> String {
    let settings = state.display_settings.lock().unwrap();
    let w = settings
        .width
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".to_string());
    let h = settings
        .height
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".to_string());
    let s = settings
        .stride
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".to_string());
    format!("{}x{} stride:{} offset:{:+}", w, h, s, settings.row_offset)
}

/// Get the current display settings for use in streaming
#[allow(clippy::missing_panics_doc)]
pub fn get_current_display_settings(state: &AppState) -> DisplaySettings {
    let settings = state.display_settings.lock().unwrap();
    let stride_index = state.stride_index.lock().unwrap();
    let _width_index = state.width_index.lock().unwrap();

    // Calculate stride if stride multiplier is set
    let stride = if let Some(si) = *stride_index {
        let multiplier = STRIDE_OPTIONS[si];
        let width = settings.width.unwrap_or(1280);
        Some((width as f32 * multiplier) as u32)
    } else {
        settings.stride
    };

    DisplaySettings {
        width: settings.width,
        height: settings.height,
        stride,
        row_offset: settings.row_offset,
    }
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

    // Create shared state for camera frames and display settings
    let frame_buffer = Arc::new(Mutex::new(FrameBuffer::default()));
    let display_settings = Arc::new(Mutex::new(DisplaySettings::default()));
    let width_index = Arc::new(Mutex::new(None));
    let height_index = Arc::new(Mutex::new(None));
    let stride_index = Arc::new(Mutex::new(None));
    let offset_index = Arc::new(Mutex::new(0usize));

    // Clone Arcs for the setup closure (used in Android USB handler)
    #[allow(unused_variables)]
    let display_settings_clone = Arc::clone(&display_settings);
    #[allow(unused_variables)]
    let width_index_clone = Arc::clone(&width_index);
    #[allow(unused_variables)]
    let stride_index_clone = Arc::clone(&stride_index);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            frame_buffer: Arc::clone(&frame_buffer),
            display_settings,
            width_index,
            height_index,
            stride_index,
            offset_index,
        })
        .invoke_handler(tauri::generate_handler![
            get_build_info,
            check_usb_status,
            cycle_resolution,
            get_resolutions,
            get_frame,
            get_frame_info,
            dump_frame,
            cycle_width,
            cycle_height,
            cycle_stride,
            cycle_offset,
            get_display_settings,
        ])
        .setup(move |_app| {
            log::info!("Tauri app setup complete");

            // On Android, we'll initialize the USB handling here
            #[cfg(target_os = "android")]
            {
                let app_handle = _app.handle().clone();
                let frame_buffer_clone = Arc::clone(&frame_buffer);
                let display_settings_usb = Arc::clone(&display_settings_clone);
                let width_index_usb = Arc::clone(&width_index_clone);
                let stride_index_usb = Arc::clone(&stride_index_clone);
                std::thread::spawn(move || {
                    usb::init_usb_handler(
                        app_handle,
                        frame_buffer_clone,
                        display_settings_usb,
                        width_index_usb,
                        stride_index_usb,
                    );
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
