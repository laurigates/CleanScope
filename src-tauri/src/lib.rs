//! `CleanScope` - Privacy-respecting USB endoscope viewer
//!
//! This module contains the core Tauri application logic and USB camera handling.

mod capture;
pub mod frame_validation;
pub mod replay;
mod usb;
pub mod yuv_conversion;

pub mod frame_assembler;
pub mod test_utils;

#[cfg(target_os = "android")]
mod libusb_android;

pub use frame_validation::ValidationLevel;

use frame_assembler::is_jpeg_data;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

use thiserror::Error;

/// Unified error type for Tauri commands.
///
/// Provides structured error handling with consistent error messages
/// and the ability to match on error types for better debugging.
#[derive(Debug, Error)]
pub enum AppError {
    /// Mutex lock was poisoned (another thread panicked while holding it)
    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),

    /// IO error during file operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Packet capture error
    #[error("Capture error: {0}")]
    Capture(#[from] capture::CaptureError),

    /// Frame is empty or not available
    #[error("No frame available")]
    NoFrame,

    /// Path resolution error (e.g., could not get cache dir)
    #[error("Path error: {0}")]
    PathError(String),
}

// Tauri requires errors to be serializable for IPC
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Helper macro to convert mutex lock errors to `AppError`
macro_rules! lock_or_err {
    ($mutex:expr) => {
        $mutex
            .lock()
            .map_err(|e| AppError::LockPoisoned(e.to_string()))
    };
}

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
    /// Whether to capture raw frame data (disabled by default to save ~54MB/s at 30fps 720p)
    pub capture_raw_frames: bool,
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self {
            frame: Vec::new(),
            raw_frame: Vec::new(),
            timestamp: Instant::now(),
            width: 0,
            height: 0,
            capture_raw_frames: false,
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

/// Consolidated display configuration
///
/// Groups `DisplaySettings` with the index values used for cycling through
/// predefined width/height/stride options. This reduces the number of separate
/// `Arc<Mutex<>>` fields in `AppState` and ensures related display state is
/// updated atomically.
#[derive(Debug, Clone, Default)]
pub struct DisplayConfig {
    /// Current display settings (width, height, stride overrides)
    pub settings: DisplaySettings,
    /// Current width option index (None = auto)
    pub width_index: Option<usize>,
    /// Current height option index (None = auto)
    pub height_index: Option<usize>,
    /// Current stride option index (None = auto)
    pub stride_index: Option<usize>,
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
    /// Consolidated display configuration (settings + cycling indexes)
    pub display: Arc<Mutex<DisplayConfig>>,
    /// Streaming configuration (MJPEG skip, YUV format)
    pub streaming_config: Arc<Mutex<StreamingConfig>>,
    /// Packet capture state for debugging
    pub capture_state: Arc<capture::CaptureState>,
    /// Flag to signal USB streaming should stop (for graceful shutdown)
    pub usb_stop_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Frame validation level (cached from env var at startup, immutable)
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
fn get_frame(state: State<'_, AppState>) -> Result<tauri::ipc::Response, AppError> {
    let buffer = lock_or_err!(state.frame_buffer)?;

    if buffer.frame.is_empty() {
        return Err(AppError::NoFrame);
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
/// Automatically disables raw frame capture after dumping to save memory bandwidth.
#[tauri::command]
fn dump_frame(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<CapturedFrame, String> {
    use std::io::Write;

    let mut buffer = state
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

    let (format_hint, raw_extension) = if is_jpeg_data(analysis_data) {
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
    let processed_ext = if is_jpeg_data(&buffer.frame) {
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

    // Capture values before clearing
    let frame_size = buffer.frame.len();
    let raw_size = buffer.raw_frame.len();
    let width = buffer.width;
    let height = buffer.height;

    // Disable raw capture and clear raw frame buffer to save memory
    buffer.capture_raw_frames = false;
    buffer.raw_frame.clear();
    log::info!("Raw frame capture disabled after dump");

    Ok(CapturedFrame {
        path: processed_filepath.to_string_lossy().to_string(),
        raw_path,
        size: frame_size,
        raw_size,
        header_hex,
        format_hint: format_hint.to_string(),
        width,
        height,
    })
}

/// Get frame metadata (dimensions and format)
#[tauri::command]
fn get_frame_info(state: State<'_, AppState>) -> Result<FrameInfo, AppError> {
    let buffer = lock_or_err!(state.frame_buffer)?;

    if buffer.frame.is_empty() {
        return Err(AppError::NoFrame);
    }

    // Detect format based on JPEG signature
    let format = if is_jpeg_data(&buffer.frame) {
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

/// Cycle through options: None -> 0 -> 1 -> ... -> N-1 -> None
fn cycle_index(current: &mut Option<usize>, max_len: usize) -> Option<usize> {
    let new_index = match *current {
        None => Some(0),
        Some(i) if i + 1 < max_len => Some(i + 1),
        Some(_) => None,
    };
    *current = new_index;
    new_index
}

/// Cycle through width options
#[tauri::command]
fn cycle_width(state: State<'_, AppState>) -> Result<String, AppError> {
    let mut display = lock_or_err!(state.display)?;

    let new_index = cycle_index(&mut display.width_index, WIDTH_OPTIONS.len());
    display.settings.width = new_index.map(|i| WIDTH_OPTIONS[i]);

    Ok(match new_index {
        None => "W:Auto".to_string(),
        Some(i) => format!("W:{}", WIDTH_OPTIONS[i]),
    })
}

/// Cycle through height options
#[tauri::command]
fn cycle_height(state: State<'_, AppState>) -> Result<String, AppError> {
    let mut display = lock_or_err!(state.display)?;

    let new_index = cycle_index(&mut display.height_index, HEIGHT_OPTIONS.len());
    display.settings.height = new_index.map(|i| HEIGHT_OPTIONS[i]);

    Ok(match new_index {
        None => "H:Auto".to_string(),
        Some(i) => format!("H:{}", HEIGHT_OPTIONS[i]),
    })
}

/// Cycle through stride options
#[tauri::command]
fn cycle_stride(state: State<'_, AppState>) -> Result<String, AppError> {
    let mut display = lock_or_err!(state.display)?;

    let new_index = cycle_index(&mut display.stride_index, STRIDE_OPTIONS.len());

    Ok(match new_index {
        None => "S:Auto".to_string(),
        Some(i) => format!("S:x{:.3}", STRIDE_OPTIONS[i]),
    })
}

/// Get current display settings as a summary string
#[tauri::command]
fn get_display_settings(state: State<'_, AppState>) -> Result<String, AppError> {
    let display = lock_or_err!(state.display)?;
    let w = display
        .settings
        .width
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".to_string());
    let h = display
        .settings
        .height
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".to_string());
    let s = display
        .settings
        .stride
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Auto".to_string());
    Ok(format!("{}x{} stride:{}", w, h, s))
}

/// Toggle MJPEG detection skip
/// When enabled, skips MJPEG format probing and goes straight to YUV streaming
#[tauri::command]
fn toggle_skip_mjpeg(state: State<'_, AppState>) -> Result<String, String> {
    let mut config = state
        .streaming_config
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    config.skip_mjpeg_detection = !config.skip_mjpeg_detection;
    log::info!("MJPEG skip: {}", config.skip_mjpeg_detection);
    Ok(if config.skip_mjpeg_detection {
        "MJPEG:Skip".to_string()
    } else {
        "MJPEG:Try".to_string()
    })
}

/// Enable raw frame capture for one frame
/// This enables capturing the next raw frame data for debugging/analysis.
/// After the frame is captured, call `dump_frame` to save it.
/// Automatically disables after `dump_frame` is called.
#[tauri::command]
fn enable_raw_capture(state: State<'_, AppState>) -> Result<String, String> {
    let mut buffer = state
        .frame_buffer
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    buffer.capture_raw_frames = true;
    log::info!("Raw frame capture enabled");
    Ok("Raw capture enabled".to_string())
}

/// Check if raw frame capture is enabled
#[tauri::command]
fn is_raw_capture_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    let buffer = state
        .frame_buffer
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    Ok(buffer.capture_raw_frames)
}

/// Cycle through pixel format options (YUYV / UYVY / NV12 / I420 / RGB888 / BGR888)
#[tauri::command]
fn cycle_pixel_format(state: State<'_, AppState>) -> Result<String, String> {
    let mut config = state
        .streaming_config
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    config.pixel_format = match config.pixel_format {
        PixelFormat::Yuyv => PixelFormat::Uyvy,
        PixelFormat::Uyvy => PixelFormat::Nv12,
        PixelFormat::Nv12 => PixelFormat::I420,
        PixelFormat::I420 => PixelFormat::Rgb888,
        PixelFormat::Rgb888 => PixelFormat::Bgr888,
        PixelFormat::Bgr888 => PixelFormat::Yuyv,
    };
    log::info!("Pixel format: {:?}", config.pixel_format);
    Ok(format_pixel_display(&config.pixel_format))
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
fn get_streaming_config(state: State<'_, AppState>) -> Result<(String, String), String> {
    let config = state
        .streaming_config
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    let mjpeg = if config.skip_mjpeg_detection {
        "MJPEG:Skip".to_string()
    } else {
        "MJPEG:Try".to_string()
    };
    let pixel = format_pixel_display(&config.pixel_format);
    Ok((mjpeg, pixel))
}

/// Cycle through available video formats
/// Returns the new format setting as a display string
#[tauri::command]
fn cycle_video_format(state: State<'_, AppState>) -> Result<String, String> {
    let mut config = state
        .streaming_config
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;

    if config.available_formats.is_empty() {
        // No formats discovered yet
        return Ok("FMT:Auto".to_string());
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
    Ok(result)
}

/// Get available video formats discovered from camera
#[tauri::command]
fn get_available_formats(state: State<'_, AppState>) -> Result<Vec<DiscoveredFormat>, String> {
    let config = state
        .streaming_config
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    Ok(config.available_formats.clone())
}

/// Get current video format setting
#[tauri::command]
fn get_video_format(state: State<'_, AppState>) -> Result<String, String> {
    let config = state
        .streaming_config
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;

    Ok(match config.selected_format_index {
        None => "FMT:Auto".to_string(),
        Some(idx) => {
            // Find format info
            if let Some(fmt) = config.available_formats.iter().find(|f| f.index == idx) {
                format!("FMT:{}:{}", fmt.index, fmt.format_type)
            } else {
                format!("FMT:{}", idx)
            }
        }
    })
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
///
/// Computes the effective `DisplaySettings` from the consolidated `DisplayConfig`,
/// applying stride multiplier if a stride index is set.
///
/// # Errors
///
/// Returns an error if the mutex lock is poisoned.
pub fn get_current_display_settings(state: &AppState) -> Result<DisplaySettings, String> {
    let display = state
        .display
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;

    // Calculate stride if stride multiplier is set
    let stride = if let Some(si) = display.stride_index {
        let multiplier = STRIDE_OPTIONS[si];
        let width = display.settings.width.unwrap_or(1280);
        Some((width as f32 * multiplier) as u32)
    } else {
        display.settings.stride
    };

    Ok(DisplaySettings {
        width: display.settings.width,
        height: display.settings.height,
        stride,
    })
}

/// Emit a USB device event to the frontend
pub fn emit_usb_event(app: &AppHandle, connected: bool, info: Option<String>) {
    let _ = app.emit("usb-device-event", UsbStatus { connected, info });
}

/// Emit a camera frame event to the frontend
pub fn emit_camera_frame(app: &AppHandle, width: u32, height: u32) {
    let _ = app.emit("camera-frame", Resolution { width, height });
}

/// Emit frame-ready event with frame metadata
///
/// This allows the frontend to skip the `get_frame_info` IPC call
/// and only fetch the raw frame data.
pub fn emit_frame_ready(app: &AppHandle, width: u32, height: u32, is_jpeg: bool) {
    let format = if is_jpeg { "jpeg" } else { "rgb" };
    let info = FrameInfo {
        width,
        height,
        format: format.to_string(),
    };
    let _ = app.emit("frame-ready", info);
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
    let display = Arc::new(Mutex::new(DisplayConfig::default()));
    let streaming_config = Arc::new(Mutex::new(StreamingConfig::default()));
    let capture_state = Arc::new(capture::CaptureState::new());
    let usb_stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Read frame validation level from environment (default: strict)
    let validation_level = std::env::var("CLEANSCOPE_FRAME_VALIDATION")
        .map(|s| ValidationLevel::from_env_str(&s))
        .unwrap_or_default();
    log::info!("Frame validation level: {:?}", validation_level);

    // Clone Arcs for the setup closure (used in Android USB handler)
    #[allow(unused_variables)]
    let display_clone = Arc::clone(&display);
    #[allow(unused_variables)]
    let streaming_config_clone = Arc::clone(&streaming_config);
    #[allow(unused_variables)]
    let usb_stop_flag_clone = Arc::clone(&usb_stop_flag);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            frame_buffer: Arc::clone(&frame_buffer),
            display,
            streaming_config,
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
            enable_raw_capture,
            is_raw_capture_enabled,
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
                let ctx = usb::StreamingContext {
                    app_handle: _app.handle().clone(),
                    frame_buffer: Arc::clone(&frame_buffer),
                    display: Arc::clone(&display_clone),
                    streaming_config: Arc::clone(&streaming_config_clone),
                    stop_flag: Arc::clone(&usb_stop_flag_clone),
                    validation_level,
                };
                std::thread::spawn(move || {
                    usb::init_usb_handler(ctx);
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod command_tests {
    use super::*;

    /// Create a test `AppState` for unit testing
    fn create_test_state() -> AppState {
        AppState {
            frame_buffer: Arc::new(Mutex::new(FrameBuffer::default())),
            display: Arc::new(Mutex::new(DisplayConfig::default())),
            streaming_config: Arc::new(Mutex::new(StreamingConfig::default())),
            capture_state: Arc::new(capture::CaptureState::new()),
            usb_stop_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            validation_level: ValidationLevel::default(),
        }
    }

    // ========================================================================
    // Tests for pure helper functions
    // ========================================================================

    #[test]
    fn test_cycle_index_starts_at_zero() {
        let mut index = None;
        let result = cycle_index(&mut index, 5);
        assert_eq!(result, Some(0));
        assert_eq!(index, Some(0));
    }

    #[test]
    fn test_cycle_index_increments() {
        let mut index = Some(0);
        let result = cycle_index(&mut index, 5);
        assert_eq!(result, Some(1));
        assert_eq!(index, Some(1));
    }

    #[test]
    fn test_cycle_index_wraps_to_none() {
        let mut index = Some(4); // Last valid index for len=5
        let result = cycle_index(&mut index, 5);
        assert_eq!(result, None);
        assert_eq!(index, None);
    }

    #[test]
    fn test_cycle_index_full_cycle() {
        let mut index = None;
        let max_len = 3;

        // None -> 0 -> 1 -> 2 -> None
        assert_eq!(cycle_index(&mut index, max_len), Some(0));
        assert_eq!(cycle_index(&mut index, max_len), Some(1));
        assert_eq!(cycle_index(&mut index, max_len), Some(2));
        assert_eq!(cycle_index(&mut index, max_len), None);
        // Should wrap back to 0
        assert_eq!(cycle_index(&mut index, max_len), Some(0));
    }

    #[test]
    fn test_format_pixel_display_all_formats() {
        assert_eq!(format_pixel_display(&PixelFormat::Yuyv), "FMT:YUYV");
        assert_eq!(format_pixel_display(&PixelFormat::Uyvy), "FMT:UYVY");
        assert_eq!(format_pixel_display(&PixelFormat::Nv12), "FMT:NV12");
        assert_eq!(format_pixel_display(&PixelFormat::I420), "FMT:I420");
        assert_eq!(format_pixel_display(&PixelFormat::Rgb888), "FMT:RGB24");
        assert_eq!(format_pixel_display(&PixelFormat::Bgr888), "FMT:BGR24");
    }

    // ========================================================================
    // Tests for display cycling commands (testing state transitions)
    // ========================================================================

    /// Helper to simulate `cycle_width` command logic on test state
    fn test_cycle_width(state: &AppState) -> Result<String, String> {
        let mut display = state
            .display
            .lock()
            .map_err(|e| format!("Lock poisoned: {}", e))?;

        let new_index = cycle_index(&mut display.width_index, WIDTH_OPTIONS.len());
        display.settings.width = new_index.map(|i| WIDTH_OPTIONS[i]);

        Ok(match new_index {
            None => "W:Auto".to_string(),
            Some(i) => format!("W:{}", WIDTH_OPTIONS[i]),
        })
    }

    /// Helper to simulate `cycle_height` command logic on test state
    fn test_cycle_height(state: &AppState) -> Result<String, String> {
        let mut display = state
            .display
            .lock()
            .map_err(|e| format!("Lock poisoned: {}", e))?;

        let new_index = cycle_index(&mut display.height_index, HEIGHT_OPTIONS.len());
        display.settings.height = new_index.map(|i| HEIGHT_OPTIONS[i]);

        Ok(match new_index {
            None => "H:Auto".to_string(),
            Some(i) => format!("H:{}", HEIGHT_OPTIONS[i]),
        })
    }

    /// Helper to simulate `cycle_stride` command logic on test state
    fn test_cycle_stride(state: &AppState) -> Result<String, String> {
        let mut display = state
            .display
            .lock()
            .map_err(|e| format!("Lock poisoned: {}", e))?;

        let new_index = cycle_index(&mut display.stride_index, STRIDE_OPTIONS.len());

        Ok(match new_index {
            None => "S:Auto".to_string(),
            Some(i) => format!("S:x{:.3}", STRIDE_OPTIONS[i]),
        })
    }

    #[test]
    fn test_cycle_width_starts_with_first_option() {
        let state = create_test_state();
        let result = test_cycle_width(&state).unwrap();
        // First option is 1280
        assert_eq!(result, "W:1280");
    }

    #[test]
    fn test_cycle_width_cycles_through_all_options() {
        let state = create_test_state();

        // Collect all results through one full cycle
        let mut results = Vec::new();
        for _ in 0..=WIDTH_OPTIONS.len() {
            results.push(test_cycle_width(&state).unwrap());
        }

        // First should be W:1280 (first option)
        assert_eq!(results[0], "W:1280");

        // Last in the array should be W:Auto (wraps back)
        assert_eq!(results[WIDTH_OPTIONS.len()], "W:Auto");

        // Verify all WIDTH_OPTIONS appear
        for (i, &width) in WIDTH_OPTIONS.iter().enumerate() {
            assert_eq!(results[i], format!("W:{}", width));
        }
    }

    #[test]
    fn test_cycle_width_updates_display_settings() {
        let state = create_test_state();

        // Initially should be None (Auto)
        {
            let display = state.display.lock().unwrap();
            assert_eq!(display.settings.width, None);
        }

        // After first cycle, should be first option
        test_cycle_width(&state).unwrap();
        {
            let display = state.display.lock().unwrap();
            assert_eq!(display.settings.width, Some(WIDTH_OPTIONS[0]));
        }
    }

    #[test]
    fn test_cycle_height_starts_with_first_option() {
        let state = create_test_state();
        let result = test_cycle_height(&state).unwrap();
        // First option is 720
        assert_eq!(result, "H:720");
    }

    #[test]
    fn test_cycle_height_cycles_through_all_options() {
        let state = create_test_state();

        // Cycle through all options plus one to get back to Auto
        for _ in 0..HEIGHT_OPTIONS.len() {
            test_cycle_height(&state).unwrap();
        }

        // Next call should return Auto
        let result = test_cycle_height(&state).unwrap();
        assert_eq!(result, "H:Auto");
    }

    #[test]
    fn test_cycle_stride_starts_with_first_option() {
        let state = create_test_state();
        let result = test_cycle_stride(&state).unwrap();
        // First option is 2.0
        assert_eq!(result, "S:x2.000");
    }

    #[test]
    fn test_cycle_stride_cycles_through_all_options() {
        let state = create_test_state();

        // Collect all results
        let mut results = Vec::new();
        for _ in 0..=STRIDE_OPTIONS.len() {
            results.push(test_cycle_stride(&state).unwrap());
        }

        // Verify format matches expected pattern (S:x{multiplier})
        assert!(results[0].starts_with("S:x"));
        assert_eq!(results[STRIDE_OPTIONS.len()], "S:Auto");
    }

    // ========================================================================
    // Tests for streaming config commands
    // ========================================================================

    /// Helper to simulate `toggle_skip_mjpeg` command logic on test state
    fn test_toggle_skip_mjpeg(state: &AppState) -> Result<String, String> {
        let mut config = state
            .streaming_config
            .lock()
            .map_err(|e| format!("Lock poisoned: {}", e))?;
        config.skip_mjpeg_detection = !config.skip_mjpeg_detection;
        Ok(if config.skip_mjpeg_detection {
            "MJPEG:Skip".to_string()
        } else {
            "MJPEG:Try".to_string()
        })
    }

    /// Helper to simulate `cycle_pixel_format` command logic on test state
    fn test_cycle_pixel_format(state: &AppState) -> Result<String, String> {
        let mut config = state
            .streaming_config
            .lock()
            .map_err(|e| format!("Lock poisoned: {}", e))?;
        config.pixel_format = match config.pixel_format {
            PixelFormat::Yuyv => PixelFormat::Uyvy,
            PixelFormat::Uyvy => PixelFormat::Nv12,
            PixelFormat::Nv12 => PixelFormat::I420,
            PixelFormat::I420 => PixelFormat::Rgb888,
            PixelFormat::Rgb888 => PixelFormat::Bgr888,
            PixelFormat::Bgr888 => PixelFormat::Yuyv,
        };
        Ok(format_pixel_display(&config.pixel_format))
    }

    #[test]
    fn test_toggle_skip_mjpeg_toggles_state() {
        let state = create_test_state();

        // Initially false, first toggle should make it true (Skip)
        let first = test_toggle_skip_mjpeg(&state).unwrap();
        assert_eq!(first, "MJPEG:Skip");

        // Toggle again should make it false (Try)
        let second = test_toggle_skip_mjpeg(&state).unwrap();
        assert_eq!(second, "MJPEG:Try");

        // Verify they are different
        assert_ne!(first, second);
    }

    #[test]
    fn test_toggle_skip_mjpeg_updates_config() {
        let state = create_test_state();

        // Initially false
        {
            let config = state.streaming_config.lock().unwrap();
            assert!(!config.skip_mjpeg_detection);
        }

        // After toggle, should be true
        test_toggle_skip_mjpeg(&state).unwrap();
        {
            let config = state.streaming_config.lock().unwrap();
            assert!(config.skip_mjpeg_detection);
        }
    }

    #[test]
    fn test_cycle_pixel_format_cycles_through_all_formats() {
        let state = create_test_state();

        // Default is YUYV, so first cycle goes to UYVY
        let mut results = Vec::new();
        for _ in 0..6 {
            results.push(test_cycle_pixel_format(&state).unwrap());
        }

        // Should cycle through all 6 formats
        assert_eq!(results[0], "FMT:UYVY"); // YUYV -> UYVY
        assert_eq!(results[1], "FMT:NV12"); // UYVY -> NV12
        assert_eq!(results[2], "FMT:I420"); // NV12 -> I420
        assert_eq!(results[3], "FMT:RGB24"); // I420 -> RGB888
        assert_eq!(results[4], "FMT:BGR24"); // RGB888 -> BGR888
        assert_eq!(results[5], "FMT:YUYV"); // BGR888 -> YUYV (wraps)
    }

    #[test]
    fn test_cycle_pixel_format_all_unique_in_cycle() {
        let state = create_test_state();

        let formats: Vec<String> = (0..6)
            .map(|_| test_cycle_pixel_format(&state).unwrap())
            .collect();

        // All 6 should be different (cycling through 6 formats)
        let unique: std::collections::HashSet<_> = formats.iter().collect();
        assert_eq!(unique.len(), 6);
    }

    // ========================================================================
    // Tests for frame info retrieval
    // ========================================================================

    /// Helper to simulate `get_frame_info` command logic on test state
    fn test_get_frame_info(state: &AppState) -> Result<FrameInfo, String> {
        let buffer = state
            .frame_buffer
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        if buffer.frame.is_empty() {
            return Err("No frame available".to_string());
        }

        let format = if is_jpeg_data(&buffer.frame) {
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

    #[test]
    fn test_get_frame_info_returns_error_when_empty() {
        let state = create_test_state();
        let result = test_get_frame_info(&state);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "No frame available");
    }

    #[test]
    fn test_get_frame_info_returns_rgb_metadata() {
        let state = create_test_state();

        // Set up RGB frame data (not JPEG - no 0xFFD8 marker)
        {
            let mut buffer = state.frame_buffer.lock().unwrap();
            buffer.width = 640;
            buffer.height = 480;
            buffer.frame = vec![0u8; 640 * 480 * 3]; // RGB24 data
        }

        let info = test_get_frame_info(&state).unwrap();
        assert_eq!(info.width, 640);
        assert_eq!(info.height, 480);
        assert_eq!(info.format, "rgb");
    }

    #[test]
    fn test_get_frame_info_detects_jpeg_format() {
        let state = create_test_state();

        // Set up JPEG frame data with SOI marker (0xFFD8)
        {
            let mut buffer = state.frame_buffer.lock().unwrap();
            buffer.width = 1280;
            buffer.height = 720;
            // Minimal JPEG header: SOI marker + some data
            buffer.frame = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        }

        let info = test_get_frame_info(&state).unwrap();
        assert_eq!(info.width, 1280);
        assert_eq!(info.height, 720);
        assert_eq!(info.format, "jpeg");
    }

    // ========================================================================
    // Tests for get_current_display_settings (public helper function)
    // ========================================================================

    #[test]
    fn test_get_current_display_settings_default() {
        let state = create_test_state();
        let settings = get_current_display_settings(&state).unwrap();

        // All should be None/Auto by default
        assert_eq!(settings.width, None);
        assert_eq!(settings.height, None);
        assert_eq!(settings.stride, None);
    }

    #[test]
    fn test_get_current_display_settings_with_width() {
        let state = create_test_state();

        // Set width via cycling
        test_cycle_width(&state).unwrap();

        let settings = get_current_display_settings(&state).unwrap();
        assert_eq!(settings.width, Some(WIDTH_OPTIONS[0]));
    }

    #[test]
    fn test_get_current_display_settings_stride_calculation() {
        let state = create_test_state();

        // Set width first (needed for stride calculation)
        test_cycle_width(&state).unwrap(); // width = 1280

        // Set stride multiplier
        test_cycle_stride(&state).unwrap(); // stride = x2.0

        let settings = get_current_display_settings(&state).unwrap();
        // Stride should be width * 2.0 = 1280 * 2.0 = 2560
        assert_eq!(settings.stride, Some(2560));
    }

    #[test]
    fn test_get_current_display_settings_stride_uses_default_width() {
        let state = create_test_state();

        // Set stride multiplier without setting width explicitly
        test_cycle_stride(&state).unwrap(); // stride = x2.0

        let settings = get_current_display_settings(&state).unwrap();
        // Should use default width 1280: 1280 * 2.0 = 2560
        assert_eq!(settings.stride, Some(2560));
    }

    // ========================================================================
    // Tests for streaming config retrieval
    // ========================================================================

    /// Helper to simulate `get_streaming_config` command logic on test state
    fn test_get_streaming_config(state: &AppState) -> Result<(String, String), String> {
        let config = state
            .streaming_config
            .lock()
            .map_err(|e| format!("Lock poisoned: {}", e))?;
        let mjpeg = if config.skip_mjpeg_detection {
            "MJPEG:Skip".to_string()
        } else {
            "MJPEG:Try".to_string()
        };
        let pixel = format_pixel_display(&config.pixel_format);
        Ok((mjpeg, pixel))
    }

    #[test]
    fn test_get_streaming_config_default() {
        let state = create_test_state();
        let (mjpeg, pixel) = test_get_streaming_config(&state).unwrap();

        // Default: try MJPEG detection, YUYV format
        assert_eq!(mjpeg, "MJPEG:Try");
        assert_eq!(pixel, "FMT:YUYV");
    }

    #[test]
    fn test_get_streaming_config_after_changes() {
        let state = create_test_state();

        // Toggle MJPEG and cycle format
        test_toggle_skip_mjpeg(&state).unwrap();
        test_cycle_pixel_format(&state).unwrap();

        let (mjpeg, pixel) = test_get_streaming_config(&state).unwrap();
        assert_eq!(mjpeg, "MJPEG:Skip");
        assert_eq!(pixel, "FMT:UYVY");
    }

    // ========================================================================
    // Tests for display settings retrieval
    // ========================================================================

    /// Helper to simulate `get_display_settings` command logic on test state
    fn test_get_display_settings(state: &AppState) -> Result<String, String> {
        let display = state
            .display
            .lock()
            .map_err(|e| format!("Lock poisoned: {}", e))?;
        let w = display
            .settings
            .width
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Auto".to_string());
        let h = display
            .settings
            .height
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Auto".to_string());
        let s = display
            .settings
            .stride
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Auto".to_string());
        Ok(format!("{}x{} stride:{}", w, h, s))
    }

    #[test]
    fn test_get_display_settings_default() {
        let state = create_test_state();
        let result = test_get_display_settings(&state).unwrap();
        assert_eq!(result, "AutoxAuto stride:Auto");
    }

    #[test]
    fn test_get_display_settings_after_cycling() {
        let state = create_test_state();

        // Set width and height
        test_cycle_width(&state).unwrap(); // 1280
        test_cycle_height(&state).unwrap(); // 720

        let result = test_get_display_settings(&state).unwrap();
        assert_eq!(result, "1280x720 stride:Auto");
    }

    // ========================================================================
    // Tests for raw capture state
    // ========================================================================

    /// Helper to simulate `enable_raw_capture` command logic on test state
    fn test_enable_raw_capture(state: &AppState) -> Result<String, String> {
        let mut buffer = state
            .frame_buffer
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        buffer.capture_raw_frames = true;
        Ok("Raw capture enabled".to_string())
    }

    /// Helper to simulate `is_raw_capture_enabled` command logic on test state
    fn test_is_raw_capture_enabled(state: &AppState) -> Result<bool, String> {
        let buffer = state
            .frame_buffer
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        Ok(buffer.capture_raw_frames)
    }

    #[test]
    fn test_raw_capture_initially_disabled() {
        let state = create_test_state();
        let enabled = test_is_raw_capture_enabled(&state).unwrap();
        assert!(!enabled);
    }

    #[test]
    fn test_enable_raw_capture_enables_flag() {
        let state = create_test_state();

        let result = test_enable_raw_capture(&state).unwrap();
        assert_eq!(result, "Raw capture enabled");

        let enabled = test_is_raw_capture_enabled(&state).unwrap();
        assert!(enabled);
    }
}
