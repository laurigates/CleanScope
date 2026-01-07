//! `CleanScope` - Privacy-respecting USB endoscope viewer
//!
//! This module contains the core Tauri application logic and USB camera handling.

mod capture;
mod frame_validation;
pub mod replay;
mod usb;

pub mod frame_assembler;
pub mod test_utils;

#[cfg(target_os = "android")]
mod libusb_android;

pub use frame_validation::ValidationLevel;

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
}

/// Pixel format variants for video frames
/// Includes both YUV and RGB formats
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum PixelFormat {
    /// YUYV format: Y0-U-Y1-V byte order (packed YUV422, luminance first)
    #[default]
    Yuyv,
    /// UYVY format: U-Y0-V-Y1 byte order (packed YUV422, chrominance first)
    /// This is what macOS reports for many USB endoscopes
    Uyvy,
    /// NV12 format: Y plane followed by interleaved UV plane (semi-planar YUV420)
    /// Uses 1.5 bytes per pixel (12 bits)
    Nv12,
    /// I420 format: Y plane, then U plane, then V plane (planar YUV420)
    /// Uses 1.5 bytes per pixel (12 bits)
    I420,
    /// RGB888 format: R-G-B byte order (3 bytes per pixel)
    /// Direct pass-through, no conversion needed
    Rgb888,
    /// BGR888 format: B-G-R byte order (3 bytes per pixel)
    /// Requires R↔B swap for display
    Bgr888,
}

/// Streaming configuration options
#[derive(Debug, Clone, Default)]
pub struct StreamingConfig {
    /// Skip MJPEG format detection and go straight to YUV
    pub skip_mjpeg_detection: bool,
    /// Pixel format for frame conversion (YUV variants or RGB)
    pub pixel_format: PixelFormat,
    /// Selected format index (None = auto-detect, Some(n) = use format n)
    pub selected_format_index: Option<u8>,
    /// Available formats discovered from camera (`format_index`, `type_name`, resolutions)
    pub available_formats: Vec<DiscoveredFormat>,
    /// Flag to signal streaming should restart with new settings
    pub restart_requested: bool,
}

/// A discovered camera format for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredFormat {
    /// UVC format index (1-based)
    pub index: u8,
    /// Human-readable format type (e.g., "MJPEG", "YUY2")
    pub format_type: String,
    /// Available resolutions as `WxH` strings
    pub resolutions: Vec<String>,
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

/// Application state managed by Tauri
pub struct AppState {
    /// Shared frame buffer protected by mutex
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
    /// Display settings for resolution/stride overrides
    pub display_settings: Arc<Mutex<DisplaySettings>>,
    /// Streaming configuration (MJPEG skip, YUV format)
    pub streaming_config: Arc<Mutex<StreamingConfig>>,
    /// Current width option index (None = auto)
    pub width_index: Arc<Mutex<Option<usize>>>,
    /// Current height option index (None = auto)
    pub height_index: Arc<Mutex<Option<usize>>>,
    /// Current stride option index (None = auto)
    pub stride_index: Arc<Mutex<Option<usize>>>,
    /// Current offset option index
    pub offset_index: Arc<Mutex<usize>>,
    /// Packet capture state for debugging
    pub capture_state: Arc<capture::CaptureState>,
    /// Flag to signal USB streaming should stop (for graceful shutdown)
    pub usb_stop_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Frame validation level (cached from env var at startup)
    pub validation_level: ValidationLevel,
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
    format!("{}x{} stride:{}", w, h, s)
}

/// Toggle MJPEG detection skip
/// When enabled, skips MJPEG format probing and goes straight to YUV streaming
#[tauri::command]
fn toggle_skip_mjpeg(state: State<'_, AppState>) -> String {
    let mut config = state.streaming_config.lock().unwrap();
    config.skip_mjpeg_detection = !config.skip_mjpeg_detection;
    log::info!("MJPEG skip: {}", config.skip_mjpeg_detection);
    if config.skip_mjpeg_detection {
        "MJPEG:Skip".to_string()
    } else {
        "MJPEG:Try".to_string()
    }
}

/// Cycle through pixel format options (YUYV / UYVY / NV12 / I420 / RGB888 / BGR888)
#[tauri::command]
fn cycle_pixel_format(state: State<'_, AppState>) -> String {
    let mut config = state.streaming_config.lock().unwrap();
    config.pixel_format = match config.pixel_format {
        PixelFormat::Yuyv => PixelFormat::Uyvy,
        PixelFormat::Uyvy => PixelFormat::Nv12,
        PixelFormat::Nv12 => PixelFormat::I420,
        PixelFormat::I420 => PixelFormat::Rgb888,
        PixelFormat::Rgb888 => PixelFormat::Bgr888,
        PixelFormat::Bgr888 => PixelFormat::Yuyv,
    };
    log::info!("Pixel format: {:?}", config.pixel_format);
    format_pixel_display(&config.pixel_format)
}

/// Format pixel format for display
fn format_pixel_display(format: &PixelFormat) -> String {
    match format {
        PixelFormat::Yuyv => "FMT:YUYV".to_string(),
        PixelFormat::Uyvy => "FMT:UYVY".to_string(),
        PixelFormat::Nv12 => "FMT:NV12".to_string(),
        PixelFormat::I420 => "FMT:I420".to_string(),
        PixelFormat::Rgb888 => "FMT:RGB24".to_string(),
        PixelFormat::Bgr888 => "FMT:BGR24".to_string(),
    }
}

/// Get current streaming configuration
#[tauri::command]
fn get_streaming_config(state: State<'_, AppState>) -> (String, String) {
    let config = state.streaming_config.lock().unwrap();
    let mjpeg = if config.skip_mjpeg_detection {
        "MJPEG:Skip".to_string()
    } else {
        "MJPEG:Try".to_string()
    };
    let pixel = format_pixel_display(&config.pixel_format);
    (mjpeg, pixel)
}

/// Cycle through available video formats
/// Returns the new format setting as a display string
#[tauri::command]
fn cycle_video_format(state: State<'_, AppState>) -> String {
    let mut config = state.streaming_config.lock().unwrap();

    if config.available_formats.is_empty() {
        // No formats discovered yet
        return "FMT:Auto".to_string();
    }

    // Cycle: None (Auto) -> format 0 -> format 1 -> ... -> None (Auto)
    let new_index = match config.selected_format_index {
        None => Some(0usize),
        Some(current) => {
            // Find current position in available_formats
            let current_pos = config
                .available_formats
                .iter()
                .position(|f| f.index == current);
            match current_pos {
                Some(pos) if pos + 1 < config.available_formats.len() => Some(pos + 1),
                _ => None, // Wrap back to Auto
            }
        }
    };

    config.selected_format_index = new_index.map(|i| config.available_formats[i].index);

    // Signal streaming to restart with new format
    config.restart_requested = true;

    let result = match new_index {
        None => "FMT:Auto".to_string(),
        Some(i) => {
            let fmt = &config.available_formats[i];
            format!("FMT:{}:{}", fmt.index, fmt.format_type)
        }
    };

    log::info!(
        "Video format changed: {:?} -> {} (restart requested)",
        config.selected_format_index,
        result
    );
    result
}

/// Get available video formats discovered from camera
#[tauri::command]
fn get_available_formats(state: State<'_, AppState>) -> Vec<DiscoveredFormat> {
    let config = state.streaming_config.lock().unwrap();
    config.available_formats.clone()
}

/// Get current video format setting
#[tauri::command]
fn get_video_format(state: State<'_, AppState>) -> String {
    let config = state.streaming_config.lock().unwrap();

    match config.selected_format_index {
        None => "FMT:Auto".to_string(),
        Some(idx) => {
            // Find format info
            if let Some(fmt) = config.available_formats.iter().find(|f| f.index == idx) {
                format!("FMT:{}:{}", fmt.index, fmt.format_type)
            } else {
                format!("FMT:{}", idx)
            }
        }
    }
}

/// Start capturing USB packets for debugging
///
/// Begins capturing raw USB packets during streaming. The packets are stored
/// in memory until `stop_packet_capture` is called.
#[tauri::command]
fn start_packet_capture(state: State<'_, AppState>) -> Result<String, String> {
    state.capture_state.start()?;
    Ok("Packet capture started".to_string())
}

/// Stop capturing USB packets and save to files
///
/// Stops the capture, writes the captured packets to the app cache directory,
/// and returns information about the captured data.
#[tauri::command]
fn stop_packet_capture(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<capture::CaptureResult, String> {
    // Get status before stopping (for duration)
    let status = state.capture_state.status();

    // Stop capture and get packets
    let packets = state.capture_state.stop();

    if packets.is_empty() {
        return Err("No packets captured".to_string());
    }

    // Get app cache directory
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Could not get cache dir: {}", e))?;

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Could not create cache dir: {}", e))?;

    // Write capture files
    capture::write_capture_files(&cache_dir, &packets, status.duration_ms)
}

/// Get the current packet capture status
///
/// Returns information about whether capture is active and how many packets
/// have been captured so far.
#[tauri::command]
fn get_capture_status(state: State<'_, AppState>) -> capture::CaptureStatus {
    state.capture_state.status()
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
    let streaming_config = Arc::new(Mutex::new(StreamingConfig::default()));
    let width_index = Arc::new(Mutex::new(None));
    let height_index = Arc::new(Mutex::new(None));
    let stride_index = Arc::new(Mutex::new(None));
    let offset_index = Arc::new(Mutex::new(0usize));
    let capture_state = Arc::new(capture::CaptureState::new());
    let usb_stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Read frame validation level from environment (default: strict)
    let validation_level = std::env::var("CLEANSCOPE_FRAME_VALIDATION")
        .map(|s| ValidationLevel::from_env_str(&s))
        .unwrap_or_default();
    log::info!("Frame validation level: {:?}", validation_level);

    // Clone Arcs for the setup closure (used in Android USB handler)
    #[allow(unused_variables)]
    let display_settings_clone = Arc::clone(&display_settings);
    #[allow(unused_variables)]
    let streaming_config_clone = Arc::clone(&streaming_config);
    #[allow(unused_variables)]
    let width_index_clone = Arc::clone(&width_index);
    #[allow(unused_variables)]
    let stride_index_clone = Arc::clone(&stride_index);
    #[allow(unused_variables)]
    let usb_stop_flag_clone = Arc::clone(&usb_stop_flag);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            frame_buffer: Arc::clone(&frame_buffer),
            display_settings,
            streaming_config,
            width_index,
            height_index,
            stride_index,
            offset_index,
            capture_state,
            usb_stop_flag,
            validation_level,
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
            get_display_settings,
            start_packet_capture,
            stop_packet_capture,
            get_capture_status,
            toggle_skip_mjpeg,
            cycle_pixel_format,
            get_streaming_config,
            cycle_video_format,
            get_available_formats,
            get_video_format,
        ])
        .setup(move |_app| {
            log::info!("Tauri app setup complete");

            // On Android, we'll initialize the USB handling here
            #[cfg(target_os = "android")]
            {
                let app_handle = _app.handle().clone();
                let frame_buffer_clone = Arc::clone(&frame_buffer);
                let display_settings_usb = Arc::clone(&display_settings_clone);
                let streaming_config_usb = Arc::clone(&streaming_config_clone);
                let width_index_usb = Arc::clone(&width_index_clone);
                let stride_index_usb = Arc::clone(&stride_index_clone);
                let usb_stop_flag_usb = Arc::clone(&usb_stop_flag_clone);
                std::thread::spawn(move || {
                    usb::init_usb_handler(
                        app_handle,
                        frame_buffer_clone,
                        display_settings_usb,
                        streaming_config_usb,
                        width_index_usb,
                        stride_index_usb,
                        usb_stop_flag_usb,
                        validation_level,
                    );
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
