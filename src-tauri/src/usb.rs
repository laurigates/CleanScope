//! USB device handling for `CleanScope`
//!
//! This module handles USB device detection, permission management,
//! and UVC camera streaming on Android.

use std::sync::{Arc, Mutex};
use tauri::AppHandle;

#[cfg(target_os = "android")]
use tauri::Emitter;

#[cfg(target_os = "android")]
use crate::frame_assembler::is_jpeg_data;
use crate::{DisplayConfig, FrameBuffer, StreamingConfig, ValidationLevel};

/// Lock a mutex with poison recovery.
///
/// If the mutex is poisoned (a thread panicked while holding it), recovers the
/// inner value and logs a warning instead of panicking.
macro_rules! lock_or_recover {
    ($mutex:expr) => {
        match $mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("Mutex poisoned, recovering");
                poisoned.into_inner()
            }
        }
    };
}

#[cfg(target_os = "android")]
use crate::{DisplaySettings, PixelFormat};

/// Context for USB streaming operations
///
/// Groups all parameters needed for USB camera streaming into a single struct,
/// simplifying function signatures throughout the streaming pipeline.
#[derive(Clone)]
pub struct StreamingContext {
    /// Tauri app handle for emitting events
    pub app_handle: AppHandle,
    /// Shared buffer for storing processed frames
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
    /// Consolidated display configuration (settings + cycling indexes)
    pub display: Arc<Mutex<DisplayConfig>>,
    /// Streaming configuration (format selection, pixel format)
    pub streaming_config: Arc<Mutex<StreamingConfig>>,
    /// Flag to signal USB streaming should stop
    pub stop_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Frame validation level
    pub validation_level: ValidationLevel,
}

#[cfg(target_os = "android")]
use jni::{
    objects::{JClass, JObject, JValue},
    sys::jint,
    JNIEnv,
};

#[cfg(target_os = "android")]
use crate::libusb_android::{
    uvc, EndpointInfo, IsochronousStream, LibusbContext, LibusbDeviceHandle, LibusbError,
    SendableContextPtr, TransferType,
};

// YUV conversion functions are in the yuv_conversion module (platform-independent)
#[cfg(target_os = "android")]
use crate::yuv_conversion::{
    convert_bgr888_to_rgb, convert_i420_to_rgb, convert_nv12_to_rgb, convert_yuv422_to_rgb,
    pass_through_rgb888, YuvPackedFormat,
};

/// Event loop timeout for libusb event handling (100ms)
#[cfg(target_os = "android")]
const LIBUSB_EVENT_TIMEOUT_USEC: libc::suseconds_t = 100_000;

/// Spawns a thread that runs the libusb event loop.
///
/// The event loop processes asynchronous USB transfers (isochronous callbacks).
/// It runs until the stop_flag is set to true.
///
/// # Arguments
/// * `ctx_ptr` - Send-safe wrapper for the libusb context pointer
/// * `stop_flag` - Atomic flag to signal when the loop should exit
/// * `thread_name` - Name for the spawned thread (for debugging)
/// * `debug_logging` - If true, logs iteration counts for debugging
///
/// # Returns
/// JoinHandle for the spawned thread
#[cfg(target_os = "android")]
fn spawn_libusb_event_loop(
    ctx_ptr: SendableContextPtr,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
    thread_name: &'static str,
    debug_logging: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            let mut timeval = libc::timeval {
                tv_sec: 0,
                tv_usec: LIBUSB_EVENT_TIMEOUT_USEC,
            };

            let mut iteration = 0u32;
            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                unsafe {
                    let ret =
                        libusb1_sys::libusb_handle_events_timeout(ctx_ptr.as_ptr(), &mut timeval);

                    if debug_logging {
                        iteration += 1;
                        if iteration <= 5 || iteration % 50 == 0 {
                            log::debug!(
                                "[{}] Event loop iteration {}, ret={}",
                                thread_name,
                                iteration,
                                ret
                            );
                        }
                    }

                    if ret < 0 {
                        let err = LibusbError::from(ret);
                        if err != LibusbError::Interrupted {
                            log::error!("[{}] Event loop error: {}", thread_name, err);
                            break;
                        }
                    }
                }
            }

            if debug_logging {
                log::info!(
                    "[{}] Event loop exiting after {} iterations",
                    thread_name,
                    iteration
                );
            } else {
                log::info!("[{}] Event loop exiting", thread_name);
            }
        })
        .expect("Failed to spawn event loop thread")
}

/// Initialize the USB handler
/// This is called from the main thread during app setup
pub fn init_usb_handler(ctx: StreamingContext) {
    log::info!("Initializing USB handler");

    #[cfg(target_os = "android")]
    {
        // On Android, we need to get the USB file descriptor via JNI
        if let Some(fd) = get_usb_file_descriptor() {
            log::info!("USB device found with fd: {}", fd);
            crate::emit_usb_event(
                &ctx.app_handle,
                true,
                Some(format!("USB Camera (fd: {})", fd)),
            );

            // Start the camera streaming loop in a new thread
            std::thread::spawn(move || {
                run_camera_loop(fd, ctx);
            });
        } else {
            log::info!("No USB device found on startup");
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = ctx; // Suppress unused warning
        log::info!("USB handling not available on this platform");
    }
}

/// Get the device name from the USB device in the intent that launched this activity.
/// Returns the device name (e.g., "/dev/bus/usb/001/002") if launched via USB_DEVICE_ATTACHED.
#[cfg(target_os = "android")]
fn get_device_name_from_intent(env: &mut JNIEnv, activity: &JObject) -> Option<String> {
    // Get the launching intent
    let intent = env
        .call_method(activity, "getIntent", "()Landroid/content/Intent;", &[])
        .ok()?
        .l()
        .ok()?;

    if intent.is_null() {
        log::info!("No intent available");
        return None;
    }

    // Get UsbManager.EXTRA_DEVICE constant ("device")
    let extra_device_key = env
        .get_static_field(
            "android/hardware/usb/UsbManager",
            "EXTRA_DEVICE",
            "Ljava/lang/String;",
        )
        .ok()?
        .l()
        .ok()?;

    // Get the UsbDevice from intent extras
    let intent_device = env
        .call_method(
            &intent,
            "getParcelableExtra",
            "(Ljava/lang/String;)Landroid/os/Parcelable;",
            &[JValue::Object(&extra_device_key)],
        )
        .ok()?
        .l()
        .ok()?;

    if intent_device.is_null() {
        log::info!("No USB device in intent extras");
        return None;
    }

    // Get the device name from the intent's UsbDevice
    let device_name_jstring = env
        .call_method(&intent_device, "getDeviceName", "()Ljava/lang/String;", &[])
        .ok()?
        .l()
        .ok()?;

    let device_name: String = env.get_string((&device_name_jstring).into()).ok()?.into();

    log::info!("Intent has USB device: {}", device_name);
    Some(device_name)
}

/// Get USB device from UsbManager.getDeviceList(), optionally matching a specific device name.
/// The device from getDeviceList() has the proper permission context for openDevice().
#[cfg(target_os = "android")]
fn get_device_from_manager<'a>(
    env: &mut JNIEnv<'a>,
    usb_manager: &JObject,
    target_device_name: Option<&str>,
) -> Option<JObject<'a>> {
    // Get the device list as a HashMap<String, UsbDevice>
    let device_map = env
        .call_method(usb_manager, "getDeviceList", "()Ljava/util/HashMap;", &[])
        .ok()?
        .l()
        .ok()?;

    // If we have a target device name, try to get it directly from the map
    if let Some(target_name) = target_device_name {
        let key = env.new_string(target_name).ok()?;
        let device = env
            .call_method(
                &device_map,
                "get",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                &[JValue::Object(&key)],
            )
            .ok()?
            .l()
            .ok()?;

        if !device.is_null() {
            log::info!("Found target device in device list: {}", target_name);
            return Some(device);
        }
        log::warn!("Target device {} not found in device list", target_name);
    }

    // Fallback: get first available device
    let values = env
        .call_method(&device_map, "values", "()Ljava/util/Collection;", &[])
        .ok()?
        .l()
        .ok()?;

    let iterator = env
        .call_method(&values, "iterator", "()Ljava/util/Iterator;", &[])
        .ok()?
        .l()
        .ok()?;

    let has_next = env
        .call_method(&iterator, "hasNext", "()Z", &[])
        .ok()?
        .z()
        .ok()?;

    if !has_next {
        log::info!("No USB devices in device list");
        return None;
    }

    let device = env
        .call_method(&iterator, "next", "()Ljava/lang/Object;", &[])
        .ok()?
        .l()
        .ok()?;

    log::info!("Got first USB device from device list");
    Some(device)
}

/// Get the USB file descriptor from Android via JNI
#[cfg(target_os = "android")]
fn get_usb_file_descriptor() -> Option<i32> {
    use ndk_context::android_context;

    // Get the Android context
    let ctx = android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

    let mut env = vm.attach_current_thread().ok()?;

    // Get the UsbManager service
    let usb_service = env
        .get_static_field(
            "android/content/Context",
            "USB_SERVICE",
            "Ljava/lang/String;",
        )
        .ok()?
        .l()
        .ok()?;

    let usb_manager = env
        .call_method(
            &activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&usb_service)],
        )
        .ok()?
        .l()
        .ok()?;

    // Get device name from intent (if launched via USB_DEVICE_ATTACHED)
    // Then look up the device in getDeviceList() - that object has proper permission context
    let target_device_name = get_device_name_from_intent(&mut env, &activity);

    // Get the device from the device list (using the intent's device name if available)
    let device = get_device_from_manager(&mut env, &usb_manager, target_device_name.as_deref())?;

    log::info!("Attempting to open USB device");

    // Open the device and get the file descriptor
    let connection = env
        .call_method(
            &usb_manager,
            "openDevice",
            "(Landroid/hardware/usb/UsbDevice;)Landroid/hardware/usb/UsbDeviceConnection;",
            &[JValue::Object(&device)],
        )
        .ok()?
        .l()
        .ok()?;

    if connection.is_null() {
        log::error!("Failed to open USB device");
        return None;
    }

    // Get the file descriptor
    let fd = env
        .call_method(&connection, "getFileDescriptor", "()I", &[])
        .ok()?
        .i()
        .ok()?;

    log::info!("fd: {}", fd);
    Some(fd)
}

/// UVC Probe/Commit control structure (26 bytes for UVC 1.1)
#[cfg(target_os = "android")]
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
struct UvcStreamControl {
    bm_hint: u16,
    b_format_index: u8,
    b_frame_index: u8,
    dw_frame_interval: u32,
    w_key_frame_rate: u16,
    w_p_frame_rate: u16,
    w_comp_quality: u16,
    w_comp_window_size: u16,
    w_delay: u16,
    dw_max_video_frame_size: u32,
    dw_max_payload_transfer_size: u32,
}

// Compile-time check: UvcStreamControl must fit in a 26-byte UVC probe/commit control transfer
#[cfg(target_os = "android")]
const _: () = assert!(std::mem::size_of::<UvcStreamControl>() <= 26);

/// Negotiated UVC stream parameters
#[cfg(target_os = "android")]
#[derive(Debug, Clone, Copy)]
struct UvcNegotiatedParams {
    endpoint: u8,
    format_index: u8,
    frame_index: u8,
    width: u16,
    height: u16,
    max_frame_size: u32,
}

/// Configuration for UVC format detection
///
/// These values control how the app discovers and validates video formats
/// from USB Video Class devices.
#[cfg(target_os = "android")]
#[derive(Debug, Clone)]
pub struct UvcConfig {
    /// Maximum format index to try when searching for MJPEG.
    /// USB cameras may have multiple format indices (1-based).
    /// Default: 4 (covers most consumer cameras)
    pub max_format_index: u8,

    /// Number of frames to analyze before deciding if format is MJPEG.
    /// More frames = more accurate detection but slower startup.
    /// Default: 10
    pub frames_to_check_format: u32,

    /// Timeout in seconds for format detection phase.
    /// If detection takes longer, falls back to best guess.
    /// Default: 10 seconds
    pub detection_timeout_secs: u64,
}

#[cfg(target_os = "android")]
impl Default for UvcConfig {
    fn default() -> Self {
        Self {
            max_format_index: 4,
            frames_to_check_format: 10,
            detection_timeout_secs: 10,
        }
    }
}

/// Default UVC configuration
#[cfg(target_os = "android")]
const UVC_CONFIG: UvcConfig = UvcConfig {
    max_format_index: 4,
    frames_to_check_format: 10,
    detection_timeout_secs: 10,
};

/// Discover available video formats from UVC descriptors and store them in streaming config.
///
/// Returns the discovered format descriptors for further processing.
#[cfg(target_os = "android")]
fn discover_and_store_formats(
    dev: &LibusbDeviceHandle,
    streaming_config: &Arc<Mutex<StreamingConfig>>,
) -> Vec<uvc::UvcFormatInfo> {
    let formats = dev.get_format_descriptors().unwrap_or_default();
    {
        let mut config = lock_or_recover!(streaming_config);
        config.available_formats = formats
            .iter()
            .map(|f| {
                let format_type = match f.format_type {
                    uvc::UvcFormatType::Mjpeg => "MJPEG".to_string(),
                    uvc::UvcFormatType::Uncompressed => "YUY2".to_string(),
                    uvc::UvcFormatType::UncompressedRgb => "RGB24".to_string(),
                    uvc::UvcFormatType::FrameBased => "H264".to_string(),
                    uvc::UvcFormatType::Unknown(n) => format!("UNK:{}", n),
                };
                let frames: Vec<crate::DiscoveredFrame> = f
                    .frames
                    .iter()
                    .map(|fr| crate::DiscoveredFrame {
                        frame_index: fr.frame_index,
                        width: fr.width,
                        height: fr.height,
                    })
                    .collect();
                crate::DiscoveredFormat {
                    index: f.format_index,
                    format_type,
                    frames,
                }
            })
            .collect();
        log::info!(
            "Discovered {} video formats: {:?}",
            config.available_formats.len(),
            config.available_formats
        );
    }
    formats
}

/// Result of MJPEG streaming attempt
#[cfg(target_os = "android")]
enum MjpegStreamingResult {
    /// MJPEG streaming completed successfully
    Success(StreamResult),
    /// Format was not MJPEG, should try next format
    NotMjpeg,
    /// Streaming failed with an error
    Error(LibusbError),
}

/// Attempt MJPEG streaming with the specified format index.
///
/// Returns `Ok(Some(result))` if MJPEG worked, `Ok(None)` if format is not MJPEG,
/// or `Err` if streaming failed.
#[cfg(target_os = "android")]
fn try_mjpeg_streaming(
    usb_ctx: &LibusbContext,
    dev: &LibusbDeviceHandle,
    ep_info: &EndpointInfo,
    stream_ctx: &StreamingContext,
    format_index: u8,
    streaming_interface: i32,
) -> MjpegStreamingResult {
    // Start UVC streaming with this format index and frame index 1 (highest resolution)
    // Use _with_resolution to get width/height for correct frame size detection
    let params = match start_uvc_streaming_with_resolution(dev, Some(ep_info), format_index, 1) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "Failed to start streaming with format {}: {}",
                format_index,
                e
            );
            return MjpegStreamingResult::Error(e);
        }
    };
    log::info!(
        "UVC streaming started on endpoint 0x{:02x} with format {}, resolution {}x{}",
        params.endpoint,
        format_index,
        params.width,
        params.height
    );

    // Choose streaming method based on endpoint type
    let result = match ep_info.transfer_type {
        TransferType::Isochronous => {
            log::info!("Using ISOCHRONOUS transfers for video streaming");
            stream_frames_isochronous_with_format_detection(
                usb_ctx,
                dev,
                ep_info,
                stream_ctx.app_handle.clone(),
                stream_ctx.frame_buffer.clone(),
                format_index,
                params.width,
                params.height,
            )
        }
        TransferType::Bulk => {
            log::info!("Using BULK transfers for video streaming");
            stream_frames(
                dev,
                ep_info.address,
                stream_ctx.app_handle.clone(),
                stream_ctx.frame_buffer.clone(),
            )
        }
        _ => {
            log::error!(
                "Unsupported endpoint transfer type: {:?}",
                ep_info.transfer_type
            );
            Err(LibusbError::NotSupported)
        }
    };

    match result {
        Ok(FormatDetectionResult::MjpegFound) => {
            log::info!(
                "MJPEG format confirmed at index {}, streaming continues",
                format_index
            );
            MjpegStreamingResult::Success(StreamResult::Normal)
        }
        Ok(FormatDetectionResult::NotMjpeg) => {
            log::info!("Format {} is not MJPEG, trying next format", format_index);
            // Reset interface before trying next format
            let _ = dev.set_interface_alt_setting(streaming_interface, 0);
            MjpegStreamingResult::NotMjpeg
        }
        Err(e) => {
            log::warn!("Streaming error with format {}: {}", format_index, e);
            // Reset interface before trying next format
            let _ = dev.set_interface_alt_setting(streaming_interface, 0);
            MjpegStreamingResult::Error(e)
        }
    }
}

/// Start YUV fallback streaming when MJPEG is not available.
///
/// Uses format index 1 by default and selected frame index from config.
#[cfg(target_os = "android")]
fn start_yuy2_fallback(
    usb_ctx: &LibusbContext,
    dev: &LibusbDeviceHandle,
    ep_info: &EndpointInfo,
    stream_ctx: &StreamingContext,
) -> Result<StreamResult, LibusbError> {
    // Get selected frame index from config, default to 1
    let frame_idx = stream_ctx
        .streaming_config
        .lock()
        .unwrap()
        .selected_frame_index
        .unwrap_or(1);

    // Start streaming with format 1 and selected frame index
    let params = start_uvc_streaming_with_resolution(dev, Some(ep_info), 1, frame_idx)?;
    log::info!(
        "Starting YUV streaming on endpoint 0x{:02x}, resolution {}x{}",
        params.endpoint,
        params.width,
        params.height
    );

    stream_frames_yuy2(
        usb_ctx,
        dev,
        ep_info,
        stream_ctx,
        params.width as u32,
        params.height as u32,
    )
}

/// Reconnection configuration constants
#[cfg(target_os = "android")]
mod reconnect_config {
    /// Maximum number of reconnection attempts (0 = unlimited)
    pub const MAX_ATTEMPTS: u32 = 0;
    /// Initial delay before first reconnection attempt (milliseconds)
    pub const INITIAL_DELAY_MS: u64 = 1000;
    /// Maximum delay between attempts (milliseconds)
    pub const MAX_DELAY_MS: u64 = 30000;
    /// Backoff multiplier for exponential delay
    pub const BACKOFF_MULTIPLIER: f64 = 1.5;
}

/// Run the camera streaming loop with restart and reconnection support
/// This outer loop handles:
/// - Restart requests (e.g., when user changes video format)
/// - Automatic reconnection after device disconnection
#[cfg(target_os = "android")]
fn run_camera_loop(initial_fd: i32, ctx: StreamingContext) {
    use crate::DisconnectReason;
    use reconnect_config::*;

    log::info!("Starting camera loop with fd: {}", initial_fd);

    let mut current_fd = initial_fd;
    let mut disconnect_reason: Option<DisconnectReason> = None;
    let mut reconnect_attempt: u32 = 0;
    let mut current_delay_ms = INITIAL_DELAY_MS;

    loop {
        // Check if we should stop (app is closing)
        if ctx.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            log::info!("Stop flag set, exiting camera loop");
            break;
        }

        // Clear any pending restart request before starting
        {
            let mut config = lock_or_recover!(ctx.streaming_config);
            config.restart_requested = false;
        }

        match run_camera_loop_inner(current_fd, &ctx) {
            Ok(StreamResult::Normal) => {
                log::info!("Camera loop ended normally");
                disconnect_reason = Some(DisconnectReason::Normal);
                break;
            }
            Ok(StreamResult::RestartRequested) => {
                log::info!("Restarting camera loop with new settings...");
                // Reset reconnect state on successful restart
                reconnect_attempt = 0;
                current_delay_ms = INITIAL_DELAY_MS;
                // Small delay to let things settle
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            Ok(StreamResult::DeviceUnplugged) => {
                log::warn!("Device was physically unplugged");
                disconnect_reason = Some(DisconnectReason::DeviceUnplugged);
                crate::emit_usb_error(
                    &ctx.app_handle,
                    crate::UsbError {
                        error_type: DisconnectReason::DeviceUnplugged,
                        message: "USB camera was disconnected".to_string(),
                        recoverable: true,
                    },
                );
                // Fall through to reconnection logic below
            }
            Ok(StreamResult::Timeout) => {
                log::warn!("Streaming timed out - no frames received");
                disconnect_reason = Some(DisconnectReason::Timeout);
                crate::emit_usb_error(
                    &ctx.app_handle,
                    crate::UsbError {
                        error_type: DisconnectReason::Timeout,
                        message: "No video frames received - camera may be disconnected"
                            .to_string(),
                        recoverable: true,
                    },
                );
                // Fall through to reconnection logic below
            }
            Ok(StreamResult::TransferError(msg)) => {
                log::error!("USB transfer error: {}", msg);
                disconnect_reason = Some(DisconnectReason::TransferError);
                crate::emit_usb_error(
                    &ctx.app_handle,
                    crate::UsbError {
                        error_type: DisconnectReason::TransferError,
                        message: format!("USB transfer error: {}", msg),
                        recoverable: true,
                    },
                );
                // Fall through to reconnection logic below
            }
            Err(e) => {
                log::error!("Camera loop error: {}", e);
                disconnect_reason = Some(DisconnectReason::Unknown);
                crate::emit_usb_error(
                    &ctx.app_handle,
                    crate::UsbError {
                        error_type: DisconnectReason::Unknown,
                        message: format!("Camera error: {}", e),
                        recoverable: true,
                    },
                );
                // Errors also trigger reconnection
            }
        }

        // If we reach here, we need to attempt reconnection
        // (either from disconnect or error)
        reconnect_attempt += 1;

        // Check if we've exceeded max attempts (if limit is set)
        if MAX_ATTEMPTS > 0 && reconnect_attempt > MAX_ATTEMPTS {
            log::warn!(
                "Max reconnection attempts ({}) exceeded, giving up",
                MAX_ATTEMPTS
            );
            crate::emit_usb_reconnect_stopped(
                &ctx.app_handle,
                Some(format!(
                    "Gave up after {} reconnection attempts",
                    MAX_ATTEMPTS
                )),
            );
            break;
        }

        // Emit disconnect event (first attempt only)
        if reconnect_attempt == 1 {
            if let Some(ref reason) = disconnect_reason {
                crate::emit_usb_disconnect(&ctx.app_handle, reason.clone(), None);
            }
        }

        // Emit reconnecting status
        log::info!(
            "Reconnection attempt {} (delay: {}ms)",
            reconnect_attempt,
            current_delay_ms
        );
        crate::emit_usb_reconnecting(
            &ctx.app_handle,
            reconnect_attempt,
            MAX_ATTEMPTS,
            Some(format!(
                "Waiting {}s before retry...",
                current_delay_ms / 1000
            )),
        );

        // Wait with exponential backoff
        let delay = std::time::Duration::from_millis(current_delay_ms);
        let start = std::time::Instant::now();
        while start.elapsed() < delay {
            // Check stop flag during wait
            if ctx.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                log::info!("Stop flag set during reconnection wait");
                crate::emit_usb_reconnect_stopped(
                    &ctx.app_handle,
                    Some("Stopped by user".to_string()),
                );
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // Increase delay for next attempt (exponential backoff)
        current_delay_ms = ((current_delay_ms as f64) * BACKOFF_MULTIPLIER) as u64;
        if current_delay_ms > MAX_DELAY_MS {
            current_delay_ms = MAX_DELAY_MS;
        }

        // Try to get a new file descriptor
        log::info!("Attempting to acquire new USB file descriptor...");
        crate::emit_usb_reconnecting(
            &ctx.app_handle,
            reconnect_attempt,
            MAX_ATTEMPTS,
            Some("Looking for USB device...".to_string()),
        );

        match get_usb_file_descriptor() {
            Some(new_fd) => {
                log::info!(
                    "Successfully acquired new USB fd: {} (attempt {})",
                    new_fd,
                    reconnect_attempt
                );

                // Emit connected event
                crate::emit_usb_event(
                    &ctx.app_handle,
                    true,
                    Some(format!("USB Camera reconnected (fd: {})", new_fd)),
                );

                // Reset reconnection state
                current_fd = new_fd;
                reconnect_attempt = 0;
                current_delay_ms = INITIAL_DELAY_MS;
                // Note: disconnect_reason will be set by the next disconnection event

                // Continue the loop to start streaming with new fd
                continue;
            }
            None => {
                log::info!(
                    "No USB device available yet (attempt {})",
                    reconnect_attempt
                );
                // Loop back to wait and try again
                continue;
            }
        }
    }

    // Emit final disconnected event with reason when camera loop exits
    let final_reason = disconnect_reason.unwrap_or(DisconnectReason::Normal);
    log::info!(
        "Camera loop exited, emitting disconnect event with reason: {:?}",
        final_reason
    );
    crate::emit_usb_reconnect_stopped(&ctx.app_handle, None);
    crate::emit_usb_disconnect(&ctx.app_handle, final_reason, None);
}

/// Result of a streaming session
#[cfg(target_os = "android")]
enum StreamResult {
    /// Streaming ended normally (e.g., user stopped, app closing)
    Normal,
    /// Restart was requested (e.g., format change)
    RestartRequested,
    /// Device was physically unplugged during streaming
    DeviceUnplugged,
    /// Streaming timed out (no frames received)
    Timeout,
    /// USB transfer error occurred
    TransferError(String),
}

#[cfg(target_os = "android")]
fn run_camera_loop_inner(
    fd: i32,
    stream_ctx: &StreamingContext,
) -> Result<StreamResult, LibusbError> {
    // Initialize libusb context for Android (no device discovery)
    let usb_ctx = LibusbContext::new_android()?;
    log::info!("libusb context created");

    // Wrap the Android file descriptor as a libusb device handle
    let dev = usb_ctx.wrap_fd(fd)?;
    log::info!("Android FD wrapped successfully");

    // Get device descriptor to verify we have a video device
    let desc = dev.get_device_descriptor()?;
    log::info!(
        "Device: VID={:04x} PID={:04x} Class={:02x}",
        desc.vendor_id,
        desc.product_id,
        desc.device_class
    );

    // Enumerate all endpoints to understand what the device supports
    log::info!("=== Enumerating USB endpoints ===");
    let endpoint_info = dev.find_streaming_endpoint()?;
    log::info!("=== Endpoint enumeration complete ===");

    let ep_info = match endpoint_info {
        Some(info) => {
            log::info!(
                "Selected streaming endpoint: 0x{:02x} ({:?}) on interface {}.{}, maxPacket={} x{}",
                info.address,
                info.transfer_type,
                info.interface_number,
                info.alt_setting,
                info.max_packet_size,
                info.transactions_per_microframe
            );
            info
        }
        None => {
            log::error!("No streaming endpoint found in device descriptors");
            return Err(LibusbError::NotFound);
        }
    };

    // Claim the video streaming interface (typically interface 1)
    // Interface 0 is usually the control interface, interface 1 is streaming
    let streaming_interface = ep_info.interface_number as i32;
    if let Err(e) = dev.claim_interface(streaming_interface) {
        log::warn!("Could not claim interface {}: {}", streaming_interface, e);
        // Try interface 0 as fallback
        dev.claim_interface(0)?;
    }

    // Discover available formats from UVC descriptors and store in streaming config
    let formats = discover_and_store_formats(&dev, &stream_ctx.streaming_config);

    // Get user's format selection and MJPEG skip preference
    let (selected_format, selected_frame, skip_mjpeg) = {
        let config = lock_or_recover!(stream_ctx.streaming_config);
        (
            config.selected_format_index,
            config.selected_frame_index,
            config.skip_mjpeg_detection,
        )
    };
    // Default to frame index 1 if not specified
    let frame_idx = selected_frame.unwrap_or(1);

    // Determine which format(s) to try based on user selection
    if let Some(format_idx) = selected_format {
        // User explicitly selected a format - use it directly
        log::info!("Using user-selected format index: {}", format_idx);

        // Check if this is an MJPEG format
        let is_mjpeg = formats
            .iter()
            .find(|f| f.format_index == format_idx)
            .map(|f| f.format_type == uvc::UvcFormatType::Mjpeg)
            .unwrap_or(false);

        if is_mjpeg {
            // Start MJPEG streaming with selected format
            // Use _with_resolution to get width/height for correct frame size detection
            let params =
                start_uvc_streaming_with_resolution(&dev, Some(&ep_info), format_idx, frame_idx)?;
            log::info!(
                "MJPEG streaming started on endpoint 0x{:02x} with format {}, resolution {}x{}",
                params.endpoint,
                format_idx,
                params.width,
                params.height
            );

            match ep_info.transfer_type {
                TransferType::Isochronous => {
                    stream_frames_isochronous_with_format_detection(
                        &usb_ctx,
                        &dev,
                        &ep_info,
                        stream_ctx.app_handle.clone(),
                        stream_ctx.frame_buffer.clone(),
                        format_idx,
                        params.width,
                        params.height,
                    )?;
                }
                TransferType::Bulk => {
                    stream_frames(
                        &dev,
                        ep_info.address,
                        stream_ctx.app_handle.clone(),
                        stream_ctx.frame_buffer.clone(),
                    )?;
                }
                _ => {
                    log::error!("Unsupported transfer type: {:?}", ep_info.transfer_type);
                    return Err(LibusbError::NotSupported);
                }
            }
            // MJPEG streaming doesn't support restart yet - return Normal
            return Ok(StreamResult::Normal);
        } else {
            // Start YUV streaming with selected format
            let params =
                start_uvc_streaming_with_resolution(&dev, Some(&ep_info), format_idx, frame_idx)?;
            log::info!(
                "YUV streaming started on endpoint 0x{:02x}, resolution {}x{} with format {}",
                params.endpoint,
                params.width,
                params.height,
                format_idx
            );

            return stream_frames_yuy2(
                &usb_ctx,
                &dev,
                &ep_info,
                stream_ctx,
                params.width as u32,
                params.height as u32,
            );
        }
    } else if skip_mjpeg {
        log::info!("Skipping MJPEG detection (user preference), going straight to YUV");
    } else {
        // Auto-detect: Try different format indices to find MJPEG format
        // Format index 1 is not guaranteed to be MJPEG - varies by device
        for format_index in 1..=UVC_CONFIG.max_format_index {
            log::info!(
                "=== Trying format index {} of {} ===",
                format_index,
                UVC_CONFIG.max_format_index
            );

            match try_mjpeg_streaming(
                &usb_ctx,
                &dev,
                &ep_info,
                stream_ctx,
                format_index,
                streaming_interface,
            ) {
                MjpegStreamingResult::Success(result) => {
                    return Ok(result);
                }
                MjpegStreamingResult::NotMjpeg | MjpegStreamingResult::Error(_) => {
                    continue;
                }
            }
        }

        log::info!("No MJPEG format found, falling back to YUV streaming");
    }

    // YUV streaming with format index 1
    start_yuy2_fallback(&usb_ctx, &dev, &ep_info, stream_ctx)
}

/// Result of format detection during streaming
#[cfg(target_os = "android")]
#[derive(Debug, Clone, Copy, PartialEq)]
enum FormatDetectionResult {
    /// MJPEG frames detected, continue streaming
    MjpegFound,
    /// Not MJPEG format, try next format index
    NotMjpeg,
}

/// Known YUY2 frame sizes for common resolutions
#[cfg(target_os = "android")]
fn detect_yuy2_resolution(frame_size: usize) -> Option<(u32, u32)> {
    // YUY2 uses 2 bytes per pixel
    match frame_size {
        1843200 => Some((1280, 720)), // 720p: 1280 * 720 * 2
        921600 => Some((640, 720)),   // Half 720p width
        614400 => Some((640, 480)),   // VGA: 640 * 480 * 2
        460800 => Some((640, 360)),   // 360p: 640 * 360 * 2
        153600 => Some((320, 240)),   // QVGA: 320 * 240 * 2
        _ => {
            // Try to guess based on common aspect ratios
            // 16:9 aspect ratio
            for height in [720u32, 480, 360, 240] {
                let width = height * 16 / 9;
                if frame_size == (width * height * 2) as usize {
                    return Some((width, height));
                }
            }
            // 4:3 aspect ratio
            for height in [480u32, 360, 240] {
                let width = height * 4 / 3;
                if frame_size == (width * height * 2) as usize {
                    return Some((width, height));
                }
            }
            None
        }
    }
}

/// Stream frames using isochronous transfers with format detection
/// Returns MjpegFound if JPEG frames are detected and continues streaming,
/// or NotMjpeg if the format doesn't appear to be MJPEG
///
/// The width/height parameters are from UVC descriptor negotiation and are used
/// to calculate the correct expected frame size for YUY2 format detection.
/// MJPEG uses EOF markers and doesn't rely on frame size.
#[cfg(target_os = "android")]
fn stream_frames_isochronous_with_format_detection(
    ctx: &LibusbContext,
    dev: &LibusbDeviceHandle,
    ep_info: &EndpointInfo,
    app_handle: AppHandle,
    shared_frame_buffer: Arc<Mutex<FrameBuffer>>,
    format_index: u8,
    width: u16,
    height: u16,
) -> Result<FormatDetectionResult, LibusbError> {
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    log::info!(
        "Starting isochronous streaming with format detection (format_index={}, resolution={}x{})",
        format_index,
        width,
        height
    );

    // Emit connecting status to update frontend UI during format detection
    let _ = app_handle.emit(
        "usb-status",
        serde_json::json!({
            "status": "connecting",
            "detail": format!("Detecting format (index {})...", format_index)
        }),
    );

    // Calculate expected frame size for YUY2 (2 bytes per pixel)
    // This ensures correct frame boundary detection if the format turns out to be YUY2.
    // MJPEG uses EOF markers and doesn't rely on this size.
    let expected_yuy2_frame_size = (width as usize) * (height as usize) * 2;

    // Create the isochronous stream
    // Use calculated frame size so YUY2 detection works correctly
    // Validation is Off since we're still detecting the format
    let mut iso_stream = unsafe {
        IsochronousStream::new(
            ctx.get_context_ptr(),
            dev.get_handle_ptr(),
            ep_info.address,
            ep_info.max_packet_size,
            expected_yuy2_frame_size, // Use descriptor-based size for YUY2 detection
            None,                     // No packet capture for format detection
            crate::ValidationLevel::Off, // No validation during format detection
            width as usize,
            height as usize,
        )?
    };

    let frame_receiver = iso_stream.take_frame_receiver().ok_or(LibusbError::Other)?;
    iso_stream.start()?;

    // Spawn event loop thread
    let event_loop_handle = spawn_libusb_event_loop(
        SendableContextPtr::new(ctx.get_context_ptr()),
        iso_stream.stop_flag.clone(),
        "format-detection",
        false,
    );

    // Phase 1: Format detection - check first N frames for JPEG markers
    let detection_start = Instant::now();
    let detection_timeout = Duration::from_secs(UVC_CONFIG.detection_timeout_secs);
    let mut frames_checked = 0u32;
    let mut jpeg_frames = 0u32;
    let mut non_jpeg_frames = 0u32;

    log::info!(
        "Format detection phase: checking up to {} frames for JPEG markers",
        UVC_CONFIG.frames_to_check_format
    );

    while frames_checked < UVC_CONFIG.frames_to_check_format {
        if detection_start.elapsed() > detection_timeout {
            log::warn!(
                "Format detection timeout after {} frames ({} JPEG, {} non-JPEG)",
                frames_checked,
                jpeg_frames,
                non_jpeg_frames
            );
            break;
        }

        match frame_receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(frame_data) => {
                frames_checked += 1;

                // Check for JPEG SOI marker (0xFFD8)
                let is_jpeg = is_jpeg_data(&frame_data);

                if is_jpeg {
                    jpeg_frames += 1;
                    log::info!(
                        "Frame {}: JPEG detected ({} bytes)",
                        frames_checked,
                        frame_data.len()
                    );
                } else {
                    non_jpeg_frames += 1;
                    // Log first few bytes for debugging
                    let header: Vec<u8> = frame_data.iter().take(8).copied().collect();
                    log::info!(
                        "Frame {}: Not JPEG ({} bytes, header: {:02x?})",
                        frames_checked,
                        frame_data.len(),
                        header
                    );
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                log::warn!("Timeout waiting for frame during format detection");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::error!("Frame channel disconnected during format detection");
                iso_stream.stop();
                let _ = event_loop_handle.join();
                return Err(LibusbError::Pipe);
            }
        }
    }

    // Decide if this format is MJPEG
    // Require at least 50% JPEG frames to consider it MJPEG
    let is_mjpeg_format = jpeg_frames > 0 && jpeg_frames >= frames_checked / 2;

    log::info!(
        "Format detection complete: {} JPEG / {} total frames - {}",
        jpeg_frames,
        frames_checked,
        if is_mjpeg_format {
            "MJPEG CONFIRMED"
        } else {
            "NOT MJPEG"
        }
    );

    if !is_mjpeg_format {
        // Not MJPEG, stop streaming and return
        iso_stream.stop();
        let _ = event_loop_handle.join();
        return Ok(FormatDetectionResult::NotMjpeg);
    }

    // Phase 2: MJPEG confirmed, continue streaming
    log::info!(
        "MJPEG format confirmed at index {}, continuing to stream",
        format_index
    );

    // Emit connected event to update frontend UI
    crate::emit_usb_event(
        &app_handle,
        true,
        Some(format!("MJPEG Camera (format {})", format_index)),
    );

    // Emit status update to frontend
    let _ = app_handle.emit(
        "usb-status",
        serde_json::json!({
            "status": "streaming",
            "detail": format!("MJPEG format (index {})", format_index)
        }),
    );

    let mut frame_count = frames_checked;

    loop {
        match frame_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(frame_data) => {
                frame_count += 1;

                // Store frame in shared buffer
                {
                    let mut buffer = lock_or_recover!(shared_frame_buffer);
                    buffer.frame = frame_data;
                    buffer.timestamp = Instant::now();
                }

                // Emit notification to trigger frontend fetch
                let _ = app_handle.emit("frame-ready", ());

                if frame_count % 30 == 0 {
                    log::info!("Received {} frames via isochronous transfer", frame_count);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                log::warn!("No frames received in 5 seconds");
                if iso_stream.is_stopped() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("Frame channel disconnected, exiting");
                break;
            }
        }
    }

    iso_stream.stop();
    let _ = event_loop_handle.join();

    log::info!("Streaming ended after {} total frames", frame_count);
    Ok(FormatDetectionResult::MjpegFound)
}

/// Stream frames using isochronous transfers (legacy, for backwards compatibility)
#[cfg(target_os = "android")]
#[allow(dead_code)]
fn stream_frames_isochronous(
    ctx: &LibusbContext,
    dev: &LibusbDeviceHandle,
    ep_info: &EndpointInfo,
    app_handle: AppHandle,
    shared_frame_buffer: Arc<Mutex<FrameBuffer>>,
) -> Result<(), LibusbError> {
    use std::time::Instant;
    use tauri::Emitter;

    log::info!(
        "Starting isochronous streaming on endpoint 0x{:02x}, max_packet={}",
        ep_info.address,
        ep_info.max_packet_size
    );

    // Create the isochronous stream
    // SAFETY: We hold references to ctx and dev for the duration of streaming
    // For legacy MJPEG streaming, we use 0 as expected frame size (MJPEG uses EOF markers)
    // Validation is Off for MJPEG since we don't do YUY2 row validation on JPEG data
    let mut iso_stream = unsafe {
        IsochronousStream::new(
            ctx.get_context_ptr(),
            dev.get_handle_ptr(),
            ep_info.address,
            ep_info.max_packet_size,
            0,                           // MJPEG uses EOF markers, not frame size
            None,                        // No packet capture for legacy streaming
            crate::ValidationLevel::Off, // No YUY2 validation for MJPEG
            0,                           // Width not used for MJPEG
            0,                           // Height not used for MJPEG
        )?
    };

    // Get the frame receiver before starting
    let frame_receiver = iso_stream.take_frame_receiver().ok_or(LibusbError::Other)?;

    // Start the transfers
    iso_stream.start()?;

    // Spawn a thread to run the libusb event loop
    let event_loop_handle = spawn_libusb_event_loop(
        SendableContextPtr::new(ctx.get_context_ptr()),
        iso_stream.stop_flag.clone(),
        "legacy-streaming",
        true, // Enable debug logging for legacy streaming path
    );

    // Process received frames and emit to frontend
    let mut frame_count = 0u32;

    loop {
        match frame_receiver.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(frame_data) => {
                frame_count += 1;

                // Store frame in shared buffer
                {
                    let mut buffer = lock_or_recover!(shared_frame_buffer);
                    buffer.frame = frame_data;
                    buffer.timestamp = Instant::now();
                }

                // Emit lightweight notification to trigger frontend fetch
                let _ = app_handle.emit("frame-ready", ());

                if frame_count % 30 == 0 {
                    log::info!("Received {} frames via isochronous transfer", frame_count);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                log::warn!("No frames received in 5 seconds");
                // Check if we should continue
                if iso_stream.is_stopped() {
                    log::info!("Stream stopped, exiting frame loop");
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("Frame channel disconnected, exiting");
                break;
            }
        }
    }

    // Stop the stream
    iso_stream.stop();

    // Wait for event loop thread to finish
    let _ = event_loop_handle.join();

    log::info!("Isochronous streaming ended after {} frames", frame_count);
    Ok(())
}

/// Calculated frame dimensions from raw frame data
#[cfg(target_os = "android")]
struct FrameDimensions {
    width: u32,
    height: u32,
    stride: u32,
    /// The actual width derived from frame data (before any overrides)
    actual_width: u32,
    /// The actual stride derived from frame data (before any overrides)
    actual_stride: u32,
}

/// Calculate frame dimensions from raw frame data and display settings
///
/// Handles stride/width/height calculation based on actual frame data size,
/// descriptor values, display settings overrides, and stride index multipliers.
/// Some cameras send more data than their descriptor claims (e.g., 1920px wide
/// despite advertising 640px). This function uses the actual frame size to
/// determine the real dimensions while respecting user overrides.
#[cfg(target_os = "android")]
fn calculate_frame_dimensions(
    frame_size: usize,
    base_width: u32,
    base_height: u32,
    settings: &DisplaySettings,
    stride_idx: &Option<usize>,
) -> FrameDimensions {
    // Use height from settings override or descriptor
    let height = settings.height.unwrap_or(base_height);

    // Calculate actual stride from frame data
    // stride = frame_size / height, ensure even for YUY2
    let actual_stride = if frame_size > 0 && height > 0 {
        let raw_stride = (frame_size as u32) / height;
        (raw_stride / 2) * 2 // Round down to even
    } else {
        base_width * 2
    };

    // Derive actual width from stride (YUY2 = 2 bytes per pixel)
    let actual_width = actual_stride / 2;

    // Use settings override or calculated values
    let width = settings.width.unwrap_or(actual_width);

    // Calculate stride: use stride_index multiplier if set, otherwise settings.stride or auto
    let stride = if let Some(si) = *stride_idx {
        // Stride index is set - use multiplier
        let multiplier = crate::STRIDE_OPTIONS[si];
        ((width as f32 * multiplier) as u32 / 2) * 2 // Round to even
    } else {
        settings.stride.unwrap_or(actual_stride)
    };

    FrameDimensions {
        width,
        height,
        stride,
        actual_width,
        actual_stride,
    }
}

/// Convert frame data to RGB based on pixel format
///
/// Dispatches to the appropriate conversion function based on the pixel format.
/// Supports YUV422 packed (YUYV/UYVY), YUV420 planar (I420/NV12), and RGB formats.
#[cfg(target_os = "android")]
fn convert_frame_to_rgb(
    frame_data: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    pixel_format: PixelFormat,
) -> Result<Vec<u8>, String> {
    let stride_override = Some(stride);

    let result = match pixel_format {
        PixelFormat::Yuyv => convert_yuv422_to_rgb(
            frame_data,
            width,
            height,
            stride_override,
            YuvPackedFormat::Yuyv,
        ),
        PixelFormat::Uyvy => convert_yuv422_to_rgb(
            frame_data,
            width,
            height,
            stride_override,
            YuvPackedFormat::Uyvy,
        ),
        PixelFormat::I420 => convert_i420_to_rgb(frame_data, width, height),
        PixelFormat::Nv12 => convert_nv12_to_rgb(frame_data, width, height),
        PixelFormat::Rgb888 => pass_through_rgb888(frame_data, width, height),
        PixelFormat::Bgr888 => convert_bgr888_to_rgb(frame_data, width, height),
    };

    // Convert ConversionError to String for backward compatibility
    result.map_err(|e| e.0)
}

/// Log detailed frame analysis for the first few frames to aid debugging.
#[cfg(target_os = "android")]
fn log_frame_analysis(
    frame_count: u32,
    frame_data: &[u8],
    base_width: u32,
    base_height: u32,
) {
    let frame_size = frame_data.len();
    let expected_size = (base_width * base_height * 2) as usize;
    let calculated_stride = if frame_size > 0 && base_height > 0 {
        frame_size as u32 / base_height
    } else {
        0
    };
    let min_stride = base_width * 2;
    let implied_width = calculated_stride / 2; // YUY2 = 2 bytes per pixel

    log::info!(
        "Frame {} analysis: size={} bytes, expected={}, stride_calc={} (implies {}px width), descriptor={}x{}",
        frame_count, frame_size, expected_size, calculated_stride, implied_width, base_width, base_height
    );

    // Warn if camera is sending more data than expected
    if frame_size > expected_size {
        let ratio = frame_size as f32 / expected_size as f32;
        log::warn!(
            "Camera sending {}x more data than descriptor! Actual stride={}, expected={}",
            ratio, calculated_stride, min_stride
        );
    }

    // Log first 16 bytes
    if frame_size >= 16 {
        log::info!(
            "Frame {} header: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
            frame_count,
            frame_data[0], frame_data[1], frame_data[2], frame_data[3],
            frame_data[4], frame_data[5], frame_data[6], frame_data[7],
            frame_data[8], frame_data[9], frame_data[10], frame_data[11],
            frame_data[12], frame_data[13], frame_data[14], frame_data[15]
        );
    }
}

/// Store a converted RGB frame in the shared buffer and notify the frontend.
#[cfg(target_os = "android")]
fn store_frame_and_emit(
    stream_ctx: &StreamingContext,
    rgb_data: Vec<u8>,
    raw_frame_data: &[u8],
    width: u32,
    height: u32,
    is_jpeg: bool,
) {
    // Log RGB buffer size once
    static RGB_LOGGED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if !RGB_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        let expected_rgb = (width * height * 3) as usize;
        log::info!(
            "RGB buffer: {} bytes (expected {}, {}x{})",
            rgb_data.len(),
            expected_rgb,
            width,
            height
        );
    }

    {
        let mut buffer = lock_or_recover!(stream_ctx.frame_buffer);
        buffer.frame = rgb_data;
        if buffer.capture_raw_frames {
            buffer.raw_frame = raw_frame_data.to_vec();
        }
        buffer.timestamp = std::time::Instant::now();
        buffer.width = width;
        buffer.height = height;
    }

    crate::emit_frame_ready(&stream_ctx.app_handle, width, height, is_jpeg);
}

/// Stream YUV 4:2:2 frames using isochronous transfers with RGB conversion
/// Supports both YUYV and UYVY formats based on streaming config
/// width/height: The negotiated resolution from UVC descriptors
/// Returns StreamResult to indicate if restart was requested
#[cfg(target_os = "android")]
fn stream_frames_yuy2(
    usb_ctx: &LibusbContext,
    dev: &LibusbDeviceHandle,
    ep_info: &EndpointInfo,
    stream_ctx: &StreamingContext,
    descriptor_width: u32,
    descriptor_height: u32,
) -> Result<StreamResult, LibusbError> {
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    // Get current pixel format to determine expected frame size
    let pixel_format = {
        let config = lock_or_recover!(stream_ctx.streaming_config);
        config.pixel_format
    };

    // Calculate expected frame size based on format
    // YUV422 (YUYV/UYVY): 2 bytes per pixel
    // YUV420 (I420/NV12): 1.5 bytes per pixel
    // RGB (RGB888/BGR888): 3 bytes per pixel
    let bytes_per_pixel = match pixel_format {
        PixelFormat::Yuyv | PixelFormat::Uyvy => 2.0,
        PixelFormat::I420 | PixelFormat::Nv12 => 1.5,
        PixelFormat::Rgb888 | PixelFormat::Bgr888 => 3.0,
    };
    let expected_frame_size =
        ((descriptor_width * descriptor_height) as f64 * bytes_per_pixel) as usize;

    let format_name = match pixel_format {
        PixelFormat::Yuyv => "YUYV",
        PixelFormat::Uyvy => "UYVY",
        PixelFormat::I420 => "I420",
        PixelFormat::Nv12 => "NV12",
        PixelFormat::Rgb888 => "RGB24",
        PixelFormat::Bgr888 => "BGR24",
    };

    log::info!(
        "Starting {} streaming with RGB conversion, descriptor resolution: {}x{}, expected frame size: {} bytes",
        format_name,
        descriptor_width,
        descriptor_height,
        expected_frame_size
    );

    // Emit connected event to update frontend UI
    crate::emit_usb_event(
        &stream_ctx.app_handle,
        true,
        Some(format!("{} Camera", format_name)),
    );

    // Create the isochronous stream with descriptor-based frame size
    let mut iso_stream = unsafe {
        IsochronousStream::new(
            usb_ctx.get_context_ptr(),
            dev.get_handle_ptr(),
            ep_info.address,
            ep_info.max_packet_size,
            expected_frame_size,
            None, // No packet capture (can be enabled for E2E testing)
            stream_ctx.validation_level,
            descriptor_width as usize,
            descriptor_height as usize,
        )?
    };

    let frame_receiver = iso_stream.take_frame_receiver().ok_or(LibusbError::Other)?;
    iso_stream.start()?;

    // Spawn event loop thread
    let event_loop_handle = spawn_libusb_event_loop(
        SendableContextPtr::new(usb_ctx.get_context_ptr()),
        iso_stream.stop_flag.clone(),
        "yuy2-streaming",
        false,
    );

    // Emit status update to frontend
    let _ = stream_ctx.app_handle.emit(
        "usb-status",
        serde_json::json!({
            "status": "streaming",
            "detail": "YUY2 format (converting to RGB)"
        }),
    );

    let mut frame_count = 0u32;

    // Use descriptor resolution - this is the authoritative source
    let base_width = descriptor_width;
    let base_height = descriptor_height;

    // Calculate minimum acceptable frame size based on format
    // YUV422: width*height*2, YUV420: width*height*1.5, RGB: width*height*3
    let min_expected_size = match pixel_format {
        PixelFormat::Yuyv | PixelFormat::Uyvy => (base_width * base_height * 2) as usize,
        PixelFormat::I420 | PixelFormat::Nv12 => ((base_width * base_height * 3) / 2) as usize,
        PixelFormat::Rgb888 | PixelFormat::Bgr888 => (base_width * base_height * 3) as usize,
    };

    loop {
        // Check restart flag and read current pixel format in a single lock
        let pixel_format = {
            let config = lock_or_recover!(stream_ctx.streaming_config);
            if config.restart_requested {
                log::info!("Restart requested, stopping YUY2 streaming");
                iso_stream.stop();
                let _ = event_loop_handle.join();
                return Ok(StreamResult::RestartRequested);
            }
            config.pixel_format
        };

        match frame_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(frame_data) => {
                frame_count += 1;
                let frame_size = frame_data.len();

                // Log detailed frame analysis for first few frames
                if frame_count <= 5 {
                    log_frame_analysis(frame_count, &frame_data, base_width, base_height);
                }

                // Skip incomplete frames - must have at least minimum expected data
                if frame_size < min_expected_size / 2 {
                    if frame_count <= 10 {
                        log::debug!(
                            "Skipping incomplete frame: {} bytes (expected >= {} bytes)",
                            frame_size,
                            min_expected_size
                        );
                    }
                    continue;
                }

                // Calculate frame dimensions using helper function
                let dims = {
                    let display = lock_or_recover!(stream_ctx.display);
                    calculate_frame_dimensions(
                        frame_size,
                        base_width,
                        base_height,
                        &display.settings,
                        &display.stride_index,
                    )
                };

                let FrameDimensions {
                    width,
                    height,
                    stride,
                    actual_width,
                    actual_stride,
                } = dims;

                // Log when we detect camera sending different resolution than descriptor
                static RESOLUTION_LOGGED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if actual_width != base_width
                    && !RESOLUTION_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    log::warn!(
                        "Camera sending {}x{} (stride={}) but descriptor says {}x{}. Using actual dimensions.",
                        actual_width, height, actual_stride, base_width, base_height
                    );
                }

                // Log settings changes
                static LAST_SETTINGS: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                let settings_hash =
                    ((width as u64) << 48) | ((height as u64) << 32) | ((stride as u64) << 16);
                let last = LAST_SETTINGS.swap(settings_hash, std::sync::atomic::Ordering::Relaxed);
                if last != settings_hash {
                    log::info!("Display settings: {}x{} stride={}", width, height, stride);
                    let _ = stream_ctx.app_handle.emit(
                        "usb-status",
                        serde_json::json!({
                            "status": "streaming",
                            "detail": format!("YUY2 {}x{} stride={} → RGB", width, height, stride)
                        }),
                    );
                };

                // Convert frame to RGB and store in shared buffer
                match convert_frame_to_rgb(&frame_data, width, height, stride, pixel_format) {
                    Ok(rgb_data) => {
                        store_frame_and_emit(
                            stream_ctx, rgb_data, &frame_data, width, height, false,
                        );

                        if frame_count % 30 == 0 {
                            log::info!(
                                "Converted {} YUY2 frames to RGB ({}x{})",
                                frame_count,
                                width,
                                height
                            );
                        }
                    }
                    Err(e) => {
                        if frame_count <= 5 {
                            log::error!("YUY2 conversion error: {}", e);
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                log::warn!("No frames received in 5 seconds");
                if iso_stream.is_stopped() {
                    break;
                }
                // If stream hasn't stopped but we're timing out, it might be the device
                // Check again after a brief moment
                std::thread::sleep(std::time::Duration::from_millis(100));
                if iso_stream.is_stopped() {
                    break;
                }
                // Set timeout as the stop reason if we keep timing out
                iso_stream.set_stop_reason(crate::libusb_android::StopReason::Timeout);
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("Frame channel disconnected, exiting");
                break;
            }
        }
    }

    iso_stream.stop();
    let _ = event_loop_handle.join();

    // Determine the result based on why we stopped
    let stop_reason = iso_stream.get_stop_reason();
    log::info!(
        "YUY2 streaming ended after {} frames, stop reason: {:?}",
        frame_count,
        stop_reason
    );

    match stop_reason {
        crate::libusb_android::StopReason::DeviceUnplugged => Ok(StreamResult::DeviceUnplugged),
        crate::libusb_android::StopReason::TransferError => Ok(StreamResult::TransferError(
            "USB transfer failed".to_string(),
        )),
        crate::libusb_android::StopReason::Timeout => Ok(StreamResult::Timeout),
        _ => Ok(StreamResult::Normal),
    }
}

/// Start UVC streaming by sending probe/commit control requests
/// Returns the endpoint address on success.
#[cfg(target_os = "android")]
fn start_uvc_streaming(
    dev: &LibusbDeviceHandle,
    endpoint_info: Option<&EndpointInfo>,
    format_index: u8,
    frame_index: u8,
) -> Result<u8, LibusbError> {
    let params =
        start_uvc_streaming_with_resolution(dev, endpoint_info, format_index, frame_index)?;
    Ok(params.endpoint)
}

/// Start UVC streaming and return full negotiated parameters including resolution.
/// Looks up width/height from the UVC frame descriptors based on negotiated frame index.
#[cfg(target_os = "android")]
fn start_uvc_streaming_with_resolution(
    dev: &LibusbDeviceHandle,
    endpoint_info: Option<&EndpointInfo>,
    format_index: u8,
    frame_index: u8,
) -> Result<UvcNegotiatedParams, LibusbError> {
    log::info!(
        "Initiating UVC probe/commit sequence with format_index={}, frame_index={}",
        format_index,
        frame_index
    );

    // Get format descriptors first so we can look up resolution
    let formats = dev.get_format_descriptors().unwrap_or_default();

    // UVC probe control - request camera format
    let mut probe = UvcStreamControl::default();
    probe.bm_hint = 1; // dwFrameInterval field is valid
    probe.b_format_index = format_index; // Try specified format
    probe.b_frame_index = frame_index; // Selected resolution

    // Request type: Class request to interface, direction OUT then IN
    let request_type_out = uvc::USB_TYPE_CLASS | uvc::USB_RECIP_INTERFACE | uvc::USB_DIR_OUT;
    let request_type_in = uvc::USB_TYPE_CLASS | uvc::USB_RECIP_INTERFACE | uvc::USB_DIR_IN;

    let streaming_interface: u16 = 1;
    let control_selector = uvc::UVC_VS_PROBE_CONTROL << 8;

    // SAFETY: UvcStreamControl is #[repr(C, packed)] with no padding or invariants.
    // The mutable borrow of `probe` is not used again while `probe_bytes` is live,
    // so there is no aliasing violation.
    let probe_bytes: &mut [u8] = unsafe {
        std::slice::from_raw_parts_mut(
            &mut probe as *mut UvcStreamControl as *mut u8,
            std::mem::size_of::<UvcStreamControl>(),
        )
    };

    // SET_CUR probe control
    log::debug!("Sending UVC SET_CUR PROBE");
    dev.control_transfer(
        request_type_out,
        uvc::UVC_SET_CUR,
        control_selector,
        streaming_interface,
        probe_bytes,
        1000,
    )?;

    // GET_CUR probe control - camera returns its chosen parameters
    log::debug!("Sending UVC GET_CUR PROBE");
    let mut response = [0u8; 26];
    dev.control_transfer(
        request_type_in,
        uvc::UVC_GET_CUR,
        control_selector,
        streaming_interface,
        &mut response,
        1000,
    )?;

    log::info!("Camera probe response received");

    // Parse the response to get the negotiated parameters
    // Use read_unaligned because UvcStreamControl is packed
    let negotiated: UvcStreamControl =
        unsafe { std::ptr::read_unaligned(response.as_ptr() as *const _) };

    // Copy fields to local variables to avoid unaligned access
    let neg_format_index = negotiated.b_format_index;
    let neg_frame_index = negotiated.b_frame_index;
    let max_frame_size = negotiated.dw_max_video_frame_size;
    let max_payload = negotiated.dw_max_payload_transfer_size;
    let frame_interval = negotiated.dw_frame_interval;

    log::info!(
        "Negotiated: format={} frame={} max_frame_size={} max_payload={} frame_interval={}",
        neg_format_index,
        neg_frame_index,
        max_frame_size,
        max_payload,
        frame_interval
    );

    // Look up resolution from frame descriptors
    let mut width: u16 = 640; // Default fallback
    let mut height: u16 = 480;
    let mut found_descriptor = false;

    // Find the format that matches the negotiated format_index
    for format in &formats {
        if format.format_index == neg_format_index {
            // Find the frame that matches the negotiated frame_index
            for frame in &format.frames {
                if frame.frame_index == neg_frame_index {
                    width = frame.width;
                    height = frame.height;
                    found_descriptor = true;
                    log::info!(
                        "Resolved negotiated resolution from descriptor: {}x{} (format={}, frame={})",
                        width, height, neg_format_index, neg_frame_index
                    );
                    break;
                }
            }
            break;
        }
    }

    // Log if probe's max_frame_size differs from descriptor
    // IMPORTANT: Always trust the descriptor resolution, not the probe response.
    // Some cameras report incorrect max_frame_size in probe responses (e.g., 1843200 for 720p
    // when the camera only supports 640x480 per the descriptor). Using the wrong size causes
    // multiple frames to be concatenated, resulting in horizontal banding artifacts.
    let descriptor_frame_size = (width as u32) * (height as u32) * 2; // YUY2 = 2 bytes per pixel
    if max_frame_size != descriptor_frame_size {
        log::warn!(
            "Probe max_frame_size={} differs from descriptor {}x{} ({}). TRUSTING DESCRIPTOR.",
            max_frame_size,
            width,
            height,
            descriptor_frame_size
        );
        // Do NOT override width/height - the descriptor is authoritative
    }

    if !found_descriptor {
        log::warn!(
            "Could not find frame descriptor for format={} frame={}, using {}x{}",
            neg_format_index,
            neg_frame_index,
            width,
            height
        );
    }

    // Log raw probe response for debugging
    log::debug!("Raw probe response: {:02x?}", &response[..26]);

    // Commit the negotiated parameters
    let commit_control = uvc::UVC_VS_COMMIT_CONTROL << 8;
    log::debug!("Sending UVC SET_CUR COMMIT");
    dev.control_transfer(
        request_type_out,
        uvc::UVC_SET_CUR,
        commit_control,
        streaming_interface,
        &mut response,
        1000,
    )?;

    log::info!("UVC streaming committed");

    // Set the alternate setting to enable the streaming endpoint
    // Use the alt setting from endpoint info if available, otherwise default to 1
    let alt_setting = endpoint_info.map(|ep| ep.alt_setting as i32).unwrap_or(1);
    let streaming_interface_i32 = streaming_interface as i32;
    dev.set_interface_alt_setting(streaming_interface_i32, alt_setting)?;

    // Return the streaming endpoint address from descriptor, or default to 0x81
    let endpoint_addr = endpoint_info.map(|ep| ep.address).unwrap_or(0x81);

    Ok(UvcNegotiatedParams {
        endpoint: endpoint_addr,
        format_index: neg_format_index,
        frame_index: neg_frame_index,
        width,
        height,
        max_frame_size,
    })
}

/// Stream frames from the camera using bulk transfers
/// Note: Most endoscopes use isochronous transfers, this is a fallback
#[cfg(target_os = "android")]
fn stream_frames(
    dev: &LibusbDeviceHandle,
    endpoint: u8,
    app_handle: AppHandle,
    shared_frame_buffer: Arc<Mutex<FrameBuffer>>,
) -> Result<FormatDetectionResult, LibusbError> {
    use std::time::Instant;

    log::info!(
        "Starting bulk frame streaming from endpoint 0x{:02x}",
        endpoint
    );

    // Buffer for receiving USB data
    // USB packets are typically up to 512 bytes (full-speed) or 1024 bytes (high-speed)
    // MJPEG frames can be several KB, so we need to accumulate packets
    let mut packet_buffer = vec![0u8; 16384]; // 16KB per transfer
    let mut local_frame_buffer = Vec::with_capacity(1024 * 1024); // 1MB for frame accumulation

    let timeout_ms = 1000;
    let mut frame_count = 0u32;
    let mut jpeg_frames = 0u32;
    let mut format_confirmed = false;

    loop {
        // Perform bulk transfer to read data
        let transferred = match dev.bulk_transfer(endpoint, &mut packet_buffer, timeout_ms) {
            Ok(n) => n,
            Err(LibusbError::Timeout) => {
                log::trace!("Bulk transfer timeout");
                continue;
            }
            Err(e) => {
                log::error!("Bulk transfer error: {}", e);
                return Err(e);
            }
        };

        // Skip empty or too-small packets (UVC headers are typically 12 bytes)
        if transferred <= 12 {
            continue;
        }

        let header_len = packet_buffer[0] as usize;
        let header_flags = packet_buffer[1];
        let end_of_frame = (header_flags & 0x02) != 0;

        // Append payload data (skip header)
        if header_len < transferred {
            local_frame_buffer.extend_from_slice(&packet_buffer[header_len..transferred]);
        }

        if !end_of_frame || local_frame_buffer.is_empty() {
            continue;
        }

        frame_count += 1;

        // Check for JPEG markers (SOI: 0xFFD8)
        let is_jpeg = local_frame_buffer.len() >= 2
            && local_frame_buffer[0] == 0xFF
            && local_frame_buffer[1] == 0xD8;

        if is_jpeg {
            jpeg_frames += 1;
            log::debug!(
                "MJPEG frame {} received: {} bytes",
                frame_count,
                local_frame_buffer.len()
            );

            // Store frame in shared buffer - swap to avoid clone inside lock
            let frame_for_buffer = std::mem::take(&mut local_frame_buffer);
            {
                let mut buffer = lock_or_recover!(shared_frame_buffer);
                buffer.frame = frame_for_buffer;
                buffer.timestamp = Instant::now();
            }

            // Emit lightweight notification to trigger frontend fetch
            let _ = app_handle.emit("frame-ready", ());

            if frame_count % 30 == 0 {
                log::info!("Received {} frames", frame_count);
            }
        } else {
            log::warn!(
                "Non-JPEG frame received: {} bytes, header: {:02x?}",
                local_frame_buffer.len(),
                &local_frame_buffer[..std::cmp::min(16, local_frame_buffer.len())]
            );
            local_frame_buffer.clear();
        }

        // Format detection: check after UVC_CONFIG.frames_to_check_format frames
        if !format_confirmed && frame_count >= UVC_CONFIG.frames_to_check_format {
            let is_mjpeg = jpeg_frames > 0 && jpeg_frames >= frame_count / 2;
            log::info!(
                "Bulk format detection: {} JPEG / {} total - {}",
                jpeg_frames,
                frame_count,
                if is_mjpeg {
                    "MJPEG CONFIRMED"
                } else {
                    "NOT MJPEG"
                }
            );
            if !is_mjpeg {
                return Ok(FormatDetectionResult::NotMjpeg);
            }
            format_confirmed = true;
        }
    }
}

#[cfg(not(target_os = "android"))]
fn run_camera_loop(_fd: i32, app_handle: AppHandle, frame_buffer: Arc<Mutex<FrameBuffer>>) {
    if let Ok(replay_path) = std::env::var("CLEANSCOPE_REPLAY_PATH") {
        log::info!("Desktop replay mode: {}", replay_path);
        replay_frame_loop(app_handle, frame_buffer, &replay_path);
    } else {
        log::info!(
            "Desktop mode (no replay path set - set CLEANSCOPE_REPLAY_PATH to enable replay)"
        );
    }
}

/// Replay frames from a captured packet file for desktop testing.
///
/// This function loads packets from a binary capture file and replays them
/// through the frame assembler, updating the `FrameBuffer` and emitting events
/// just like the Android USB path does.
#[cfg(not(target_os = "android"))]
fn replay_frame_loop(
    app_handle: AppHandle,
    frame_buffer: Arc<Mutex<FrameBuffer>>,
    replay_path: &str,
) {
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    use crate::replay::{PacketReplay, ReplayConfig};

    let path = Path::new(replay_path);

    // Verify file exists
    if !path.exists() {
        log::error!("Replay file not found: {}", replay_path);
        crate::emit_usb_event(
            &app_handle,
            false,
            Some(format!("Replay file not found: {}", replay_path)),
        );
        return;
    }

    // Load packets from capture file
    let config = ReplayConfig {
        speed: 1.0,          // Real-time playback
        loop_playback: true, // Loop continuously for E2E testing
        ..Default::default()
    };

    let mut replay = match PacketReplay::load_with_config(path, config) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Failed to load replay file: {}", e);
            crate::emit_usb_event(
                &app_handle,
                false,
                Some(format!("Failed to load replay: {}", e)),
            );
            return;
        }
    };

    // Get metadata for display info
    let info = if let Some(meta) = replay.metadata() {
        format!(
            "Replay: {}x{} {} ({} packets)",
            meta.width,
            meta.height,
            meta.format_type,
            replay.packet_count()
        )
    } else {
        format!("Replay: {} packets", replay.packet_count())
    };

    // Emit connected event
    crate::emit_usb_event(&app_handle, true, Some(info));

    log::info!(
        "Starting replay: {} packets, {} ms duration",
        replay.packet_count(),
        replay.duration_ms()
    );

    // Start the replay thread and get the frame receiver
    let frame_rx = match replay.start() {
        Ok(rx) => rx,
        Err(e) => {
            log::error!("Failed to start replay: {}", e);
            crate::emit_usb_event(
                &app_handle,
                false,
                Some(format!("Failed to start replay: {}", e)),
            );
            return;
        }
    };

    let mut frame_count = 0u64;
    let start_time = Instant::now();

    // Process frames from the replay channel
    loop {
        match frame_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(frame_data) => {
                frame_count += 1;

                // Store frame in shared buffer
                {
                    let mut buffer = lock_or_recover!(frame_buffer);
                    buffer.frame = frame_data;
                    buffer.timestamp = Instant::now();
                }

                // Emit notification to trigger frontend fetch
                let _ = app_handle.emit("frame-ready", ());

                if frame_count.is_multiple_of(30) {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let fps = frame_count as f64 / elapsed;
                    log::info!("Replay: {} frames, {:.1} fps", frame_count, fps);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // No frames for 5 seconds - replay might have ended or stalled
                log::warn!("Replay timeout - no frames received for 5 seconds");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Channel closed - replay thread ended
                log::info!("Replay channel closed after {} frames", frame_count);
                break;
            }
        }
    }

    // Emit disconnected event
    crate::emit_usb_event(
        &app_handle,
        false,
        Some(format!("Replay ended ({} frames)", frame_count)),
    );
}

/// JNI callback for USB device attached events
/// This is called from Java when Android detects a USB device attachment
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_cleanscope_app_MainActivity_onUsbDeviceAttached(
    _env: JNIEnv,
    _class: JClass,
    fd: jint,
) {
    log::info!("USB Device Attached via JNI, fd: {}", fd);

    // TODO: Notify the main app about the new device
    // This would trigger the camera initialization
}

/// JNI callback for USB device detached events
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_cleanscope_app_MainActivity_onUsbDeviceDetached(
    _env: JNIEnv,
    _class: JClass,
) {
    log::info!("USB Device Detached via JNI");

    // TODO: Stop the camera stream and clean up resources
}
