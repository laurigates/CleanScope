//! USB device handling for `CleanScope`
//!
//! This module handles USB device detection, permission management,
//! and UVC camera streaming on Android.

use std::sync::{Arc, Mutex};
use tauri::AppHandle;

#[cfg(target_os = "android")]
use tauri::Emitter;

use crate::{DisplaySettings, FrameBuffer, StreamingConfig};

#[cfg(target_os = "android")]
use crate::YuvFormat;

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

#[cfg(target_os = "android")]
use yuvutils_rs::{uyvy422_to_rgb, yuyv422_to_rgb, YuvPackedImage, YuvRange, YuvStandardMatrix};

/// Initialize the USB handler
/// This is called from the main thread during app setup
pub fn init_usb_handler(
    app_handle: AppHandle,
    frame_buffer: Arc<Mutex<FrameBuffer>>,
    display_settings: Arc<Mutex<DisplaySettings>>,
    streaming_config: Arc<Mutex<StreamingConfig>>,
    width_index: Arc<Mutex<Option<usize>>>,
    stride_index: Arc<Mutex<Option<usize>>>,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
) {
    log::info!("Initializing USB handler");

    #[cfg(target_os = "android")]
    {
        // On Android, we need to get the USB file descriptor via JNI
        if let Some(fd) = get_usb_file_descriptor() {
            log::info!("USB device found with fd: {}", fd);
            crate::emit_usb_event(&app_handle, true, Some(format!("USB Camera (fd: {})", fd)));

            // Start the camera streaming loop in a new thread
            std::thread::spawn(move || {
                run_camera_loop(
                    fd,
                    app_handle,
                    frame_buffer,
                    display_settings,
                    streaming_config,
                    width_index,
                    stride_index,
                    stop_flag,
                );
            });
        } else {
            log::info!("No USB device found on startup");
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = display_settings; // Suppress unused warning
        let _ = streaming_config;
        let _ = width_index;
        let _ = stride_index;
        let _ = stop_flag;
    }

    #[cfg(not(target_os = "android"))]
    {
        log::info!("USB handling not available on this platform");
        let _ = app_handle; // Suppress unused warning
        let _ = frame_buffer; // Suppress unused warning
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

/// Maximum number of format indices to try when searching for MJPEG
#[cfg(target_os = "android")]
const MAX_FORMAT_INDEX: u8 = 4;

/// Number of frames to check before deciding if format is MJPEG
#[cfg(target_os = "android")]
const FRAMES_TO_CHECK_FORMAT: u32 = 10;

/// Timeout in seconds for format detection
#[cfg(target_os = "android")]
const FORMAT_DETECTION_TIMEOUT_SECS: u64 = 10;

/// Run the camera streaming loop with restart support
/// This outer loop handles restart requests (e.g., when user changes video format)
#[cfg(target_os = "android")]
fn run_camera_loop(
    fd: i32,
    app_handle: AppHandle,
    frame_buffer: Arc<Mutex<FrameBuffer>>,
    display_settings: Arc<Mutex<DisplaySettings>>,
    streaming_config: Arc<Mutex<StreamingConfig>>,
    width_index: Arc<Mutex<Option<usize>>>,
    stride_index: Arc<Mutex<Option<usize>>>,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
) {
    log::info!("Starting camera loop with fd: {}", fd);

    loop {
        // Check if we should stop (app is closing)
        if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            log::info!("Stop flag set, exiting camera loop");
            break;
        }

        // Clear any pending restart request before starting
        {
            let mut config = streaming_config.lock().unwrap();
            config.restart_requested = false;
        }

        match run_camera_loop_inner(
            fd,
            app_handle.clone(),
            frame_buffer.clone(),
            display_settings.clone(),
            streaming_config.clone(),
            width_index.clone(),
            stride_index.clone(),
        ) {
            Ok(StreamResult::Normal) => {
                log::info!("Camera loop ended normally");
                break;
            }
            Ok(StreamResult::RestartRequested) => {
                log::info!("Restarting camera loop with new settings...");
                // Small delay to let things settle
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                log::error!("Camera loop error: {}", e);
                break;
            }
        }
    }

    // Emit disconnected event when camera loop exits
    log::info!("Camera loop exited, emitting disconnect event");
    crate::emit_usb_event(&app_handle, false, None);
}

/// Result of a streaming session
#[cfg(target_os = "android")]
enum StreamResult {
    /// Streaming ended normally (e.g., device disconnected)
    Normal,
    /// Restart was requested (e.g., format change)
    RestartRequested,
}

#[cfg(target_os = "android")]
fn run_camera_loop_inner(
    fd: i32,
    app_handle: AppHandle,
    frame_buffer: Arc<Mutex<FrameBuffer>>,
    display_settings: Arc<Mutex<DisplaySettings>>,
    streaming_config: Arc<Mutex<StreamingConfig>>,
    width_index: Arc<Mutex<Option<usize>>>,
    stride_index: Arc<Mutex<Option<usize>>>,
) -> Result<StreamResult, LibusbError> {
    // Initialize libusb context for Android (no device discovery)
    let ctx = LibusbContext::new_android()?;
    log::info!("libusb context created");

    // Wrap the Android file descriptor as a libusb device handle
    let dev = ctx.wrap_fd(fd)?;
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
    let formats = dev.get_format_descriptors().unwrap_or_default();
    {
        let mut config = streaming_config.lock().unwrap();
        config.available_formats = formats
            .iter()
            .map(|f| {
                let format_type = match f.format_type {
                    crate::libusb_android::uvc::UvcFormatType::Mjpeg => "MJPEG".to_string(),
                    crate::libusb_android::uvc::UvcFormatType::Uncompressed => "YUY2".to_string(),
                    crate::libusb_android::uvc::UvcFormatType::FrameBased => "H264".to_string(),
                    crate::libusb_android::uvc::UvcFormatType::Unknown(n) => format!("UNK:{}", n),
                };
                let resolutions: Vec<String> = f
                    .frames
                    .iter()
                    .map(|fr| format!("{}x{}", fr.width, fr.height))
                    .collect();
                crate::DiscoveredFormat {
                    index: f.format_index,
                    format_type,
                    resolutions,
                }
            })
            .collect();
        log::info!(
            "Discovered {} video formats: {:?}",
            config.available_formats.len(),
            config.available_formats
        );
    }

    // Get user's format selection and MJPEG skip preference
    let (selected_format, skip_mjpeg) = {
        let config = streaming_config.lock().unwrap();
        (config.selected_format_index, config.skip_mjpeg_detection)
    };

    // Determine which format(s) to try based on user selection
    if let Some(format_idx) = selected_format {
        // User explicitly selected a format - use it directly
        log::info!("Using user-selected format index: {}", format_idx);

        // Check if this is an MJPEG format
        let is_mjpeg = formats
            .iter()
            .find(|f| f.format_index == format_idx)
            .map(|f| f.format_type == crate::libusb_android::uvc::UvcFormatType::Mjpeg)
            .unwrap_or(false);

        if is_mjpeg {
            // Start MJPEG streaming with selected format
            let endpoint = start_uvc_streaming(&dev, Some(&ep_info), format_idx)?;
            log::info!(
                "MJPEG streaming started on endpoint 0x{:02x} with format {}",
                endpoint,
                format_idx
            );

            match ep_info.transfer_type {
                TransferType::Isochronous => {
                    stream_frames_isochronous_with_format_detection(
                        &ctx,
                        &dev,
                        &ep_info,
                        app_handle.clone(),
                        frame_buffer.clone(),
                        format_idx,
                    )?;
                }
                TransferType::Bulk => {
                    stream_frames(&dev, endpoint, app_handle.clone(), frame_buffer.clone())?;
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
            let params = start_uvc_streaming_with_resolution(&dev, Some(&ep_info), format_idx)?;
            log::info!(
                "YUV streaming started on endpoint 0x{:02x}, resolution {}x{} with format {}",
                params.endpoint,
                params.width,
                params.height,
                format_idx
            );

            return stream_frames_yuy2(
                &ctx,
                &dev,
                &ep_info,
                app_handle,
                frame_buffer,
                display_settings,
                streaming_config,
                width_index,
                stride_index,
                params.width as u32,
                params.height as u32,
            );
        }
    } else if skip_mjpeg {
        log::info!("Skipping MJPEG detection (user preference), going straight to YUV");
    } else {
        // Auto-detect: Try different format indices to find MJPEG format
        // Format index 1 is not guaranteed to be MJPEG - varies by device
        for format_index in 1..=MAX_FORMAT_INDEX {
            log::info!(
                "=== Trying format index {} of {} ===",
                format_index,
                MAX_FORMAT_INDEX
            );

            // Start UVC streaming with this format index
            let endpoint = match start_uvc_streaming(&dev, Some(&ep_info), format_index) {
                Ok(ep) => ep,
                Err(e) => {
                    log::warn!(
                        "Failed to start streaming with format {}: {}",
                        format_index,
                        e
                    );
                    continue;
                }
            };
            log::info!(
                "UVC streaming started on endpoint 0x{:02x} with format {}",
                endpoint,
                format_index
            );

            // Choose streaming method based on endpoint type
            let result = match ep_info.transfer_type {
                TransferType::Isochronous => {
                    log::info!("Using ISOCHRONOUS transfers for video streaming");
                    stream_frames_isochronous_with_format_detection(
                        &ctx,
                        &dev,
                        &ep_info,
                        app_handle.clone(),
                        frame_buffer.clone(),
                        format_index,
                    )
                }
                TransferType::Bulk => {
                    log::info!("Using BULK transfers for video streaming");
                    stream_frames(&dev, endpoint, app_handle.clone(), frame_buffer.clone())
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
                    // MJPEG streaming doesn't support restart yet - return Normal
                    return Ok(StreamResult::Normal);
                }
                Ok(FormatDetectionResult::NotMjpeg) => {
                    log::info!("Format {} is not MJPEG, trying next format", format_index);
                    // Reset interface before trying next format
                    let _ = dev.set_interface_alt_setting(streaming_interface, 0);
                    continue;
                }
                Err(e) => {
                    log::warn!("Streaming error with format {}: {}", format_index, e);
                    // Reset interface before trying next format
                    let _ = dev.set_interface_alt_setting(streaming_interface, 0);
                    continue;
                }
            }
        }

        log::info!("No MJPEG format found, falling back to YUV streaming");
    }

    // YUV streaming with format index 1
    // Start streaming with format 1 and get negotiated resolution
    let params = start_uvc_streaming_with_resolution(&dev, Some(&ep_info), 1)?;
    log::info!(
        "Starting YUV streaming on endpoint 0x{:02x}, resolution {}x{}",
        params.endpoint,
        params.width,
        params.height
    );

    stream_frames_yuy2(
        &ctx,
        &dev,
        &ep_info,
        app_handle,
        frame_buffer,
        display_settings,
        streaming_config,
        width_index,
        stride_index,
        params.width as u32,
        params.height as u32,
    )
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

/// Calculate actual YUY2 stride from frame size and height
///
/// Some cameras use row padding for alignment (e.g., 64-byte alignment).
/// This function detects the actual stride from the frame size.
#[cfg(target_os = "android")]
fn calculate_yuy2_stride(frame_size: usize, width: u32, height: u32) -> u32 {
    let expected_stride = width * 2; // Standard: 2 bytes per pixel
    let expected_size = (expected_stride * height) as usize;

    // If frame matches expected size exactly, use standard stride
    if frame_size == expected_size {
        return expected_stride;
    }

    // Calculate actual stride from frame size
    // actual_stride = frame_size / height (rounded)
    let actual_stride = (frame_size as u32) / height;

    // Validate the calculated stride is reasonable:
    // - Must be at least width * 2 (minimum for YUY2)
    // - Should not be more than 20% larger (typical alignment padding is small)
    let max_reasonable_stride = expected_stride * 12 / 10; // 120% of expected

    if actual_stride >= expected_stride && actual_stride <= max_reasonable_stride {
        log::info!(
            "Detected YUY2 stride: {} bytes/row (expected {}, frame_size={}, height={})",
            actual_stride,
            expected_stride,
            frame_size,
            height
        );
        actual_stride
    } else if actual_stride > max_reasonable_stride {
        // Frame is much larger than expected - might be a different resolution
        log::warn!(
            "Calculated stride {} is too large (expected ~{}), using expected stride",
            actual_stride,
            expected_stride
        );
        expected_stride
    } else {
        // Frame is smaller than expected - use expected stride and truncate
        log::warn!(
            "Calculated stride {} is too small (expected {}), using expected stride",
            actual_stride,
            expected_stride
        );
        expected_stride
    }
}

/// YUV 4:2:2 packed format variant
#[cfg(target_os = "android")]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum YuvPackedFormat {
    /// YUYV format: Y0-U-Y1-V byte order (luminance first)
    #[default]
    Yuyv,
    /// UYVY format: U-Y0-V-Y1 byte order (chrominance first)
    /// This is what macOS reports for many USB endoscopes
    Uyvy,
}

/// Convert YUV 4:2:2 packed frame to RGB with automatic stride detection
///
/// This function handles cameras that use row padding for alignment.
/// Supports both YUYV and UYVY byte orders.
///
/// - `yuv_data`: Raw YUV 4:2:2 packed data
/// - `width`: Frame width in pixels
/// - `height`: Frame height in pixels
/// - `stride_override`: If Some, use this as the YUV stride instead of auto-detecting
/// - `format`: YUYV or UYVY byte order
#[cfg(target_os = "android")]
fn convert_yuv422_to_rgb(
    yuv_data: &[u8],
    width: u32,
    height: u32,
    stride_override: Option<u32>,
    format: YuvPackedFormat,
) -> Result<Vec<u8>, String> {
    let frame_size = yuv_data.len();
    let expected_stride = width * 2;

    // Use override stride if provided, otherwise auto-detect
    let actual_stride =
        stride_override.unwrap_or_else(|| calculate_yuy2_stride(frame_size, width, height));

    // Log conversion parameters when they change
    static LAST_PARAMS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let format_bit = if format == YuvPackedFormat::Uyvy {
        1u64
    } else {
        0u64
    };
    let params_hash = ((width as u64) << 48)
        | ((height as u64) << 32)
        | ((actual_stride as u64) << 16)
        | format_bit;
    let last = LAST_PARAMS.swap(params_hash, std::sync::atomic::Ordering::Relaxed);
    if last != params_hash {
        log::info!(
            "{:?} conversion: input={} bytes, width={}, height={}, stride={}",
            format,
            frame_size,
            width,
            height,
            actual_stride
        );
    }

    // Validate we have enough data
    let min_required = (expected_stride * height) as usize;
    if frame_size < min_required {
        return Err(format!(
            "YUV data too small: {} bytes, expected at least {} bytes",
            frame_size, min_required
        ));
    }

    // Calculate how much data we need with the given stride
    let actual_frame_size = (actual_stride * height) as usize;
    let data_to_use = actual_frame_size.min(frame_size);

    let packed_image = YuvPackedImage {
        yuy: &yuv_data[..data_to_use],
        yuy_stride: actual_stride,
        width,
        height,
    };

    // RGB output: 3 bytes per pixel
    let rgb_stride = width * 3;
    let mut rgb_buffer = vec![0u8; (rgb_stride * height) as usize];

    // Convert based on format - BT.601 for standard cameras, Limited range is common
    match format {
        YuvPackedFormat::Yuyv => {
            yuyv422_to_rgb(
                &packed_image,
                &mut rgb_buffer,
                rgb_stride,
                YuvRange::Limited,
                YuvStandardMatrix::Bt601,
            )
            .map_err(|e| format!("YUYV conversion error: {:?}", e))?;
        }
        YuvPackedFormat::Uyvy => {
            uyvy422_to_rgb(
                &packed_image,
                &mut rgb_buffer,
                rgb_stride,
                YuvRange::Limited,
                YuvStandardMatrix::Bt601,
            )
            .map_err(|e| format!("UYVY conversion error: {:?}", e))?;
        }
    }

    Ok(rgb_buffer)
}

/// Legacy wrapper for backward compatibility
/// Defaults to YUYV format
#[cfg(target_os = "android")]
fn convert_yuy2_to_rgb(
    yuy2_data: &[u8],
    width: u32,
    height: u32,
    stride_override: Option<u32>,
) -> Result<Vec<u8>, String> {
    convert_yuv422_to_rgb(
        yuy2_data,
        width,
        height,
        stride_override,
        YuvPackedFormat::Yuyv,
    )
}

/// Stream frames using isochronous transfers with format detection
/// Returns MjpegFound if JPEG frames are detected and continues streaming,
/// or NotMjpeg if the format doesn't appear to be MJPEG
#[cfg(target_os = "android")]
fn stream_frames_isochronous_with_format_detection(
    ctx: &LibusbContext,
    dev: &LibusbDeviceHandle,
    ep_info: &EndpointInfo,
    app_handle: AppHandle,
    shared_frame_buffer: Arc<Mutex<FrameBuffer>>,
    format_index: u8,
) -> Result<FormatDetectionResult, LibusbError> {
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    log::info!(
        "Starting isochronous streaming with format detection (format_index={})",
        format_index
    );

    // Emit connecting status to update frontend UI during format detection
    let _ = app_handle.emit(
        "usb-status",
        serde_json::json!({
            "status": "connecting",
            "detail": format!("Detecting format (index {})...", format_index)
        }),
    );

    // Create the isochronous stream
    // For format detection, we use 0 as expected frame size (MJPEG uses EOF markers, not size)
    let mut iso_stream = unsafe {
        IsochronousStream::new(
            ctx.get_context_ptr(),
            dev.get_handle_ptr(),
            ep_info.address,
            ep_info.max_packet_size,
            0, // MJPEG uses EOF markers, not frame size
        )?
    };

    let frame_receiver = iso_stream.take_frame_receiver().ok_or(LibusbError::Other)?;
    iso_stream.start()?;

    // Spawn event loop thread
    let event_loop_handle = {
        let ctx_ptr = SendableContextPtr::new(ctx.get_context_ptr());
        let stop_flag = iso_stream.stop_flag.clone();

        std::thread::spawn(move || {
            let mut timeval = libc::timeval {
                tv_sec: 0,
                tv_usec: 100_000 as libc::suseconds_t,
            };

            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                unsafe {
                    let ret =
                        libusb1_sys::libusb_handle_events_timeout(ctx_ptr.as_ptr(), &mut timeval);
                    if ret < 0 {
                        let err = LibusbError::from(ret);
                        if err != LibusbError::Interrupted {
                            log::error!("Event loop error: {}", err);
                            break;
                        }
                    }
                }
            }
            log::info!("Format detection event loop exiting");
        })
    };

    // Phase 1: Format detection - check first N frames for JPEG markers
    let detection_start = Instant::now();
    let detection_timeout = Duration::from_secs(FORMAT_DETECTION_TIMEOUT_SECS);
    let mut frames_checked = 0u32;
    let mut jpeg_frames = 0u32;
    let mut non_jpeg_frames = 0u32;

    log::info!(
        "Format detection phase: checking up to {} frames for JPEG markers",
        FRAMES_TO_CHECK_FORMAT
    );

    while frames_checked < FRAMES_TO_CHECK_FORMAT {
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
                let is_jpeg =
                    frame_data.len() >= 2 && frame_data[0] == 0xFF && frame_data[1] == 0xD8;

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
                    let mut buffer = shared_frame_buffer.lock().unwrap();
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
    let mut iso_stream = unsafe {
        IsochronousStream::new(
            ctx.get_context_ptr(),
            dev.get_handle_ptr(),
            ep_info.address,
            ep_info.max_packet_size,
            0, // MJPEG uses EOF markers, not frame size
        )?
    };

    // Get the frame receiver before starting
    let frame_receiver = iso_stream.take_frame_receiver().ok_or(LibusbError::Other)?;

    // Start the transfers
    iso_stream.start()?;

    // Spawn a thread to run the libusb event loop
    let event_loop_handle = {
        // Wrap the raw pointer in a Send-safe wrapper (uses usize internally)
        let ctx_ptr = SendableContextPtr::new(ctx.get_context_ptr());
        let stop_flag = iso_stream.stop_flag.clone();

        std::thread::spawn(move || {
            log::info!("Event loop thread started");

            let mut timeval = libc::timeval {
                tv_sec: 0,
                tv_usec: 100_000 as libc::suseconds_t, // 100ms timeout
            };

            let mut iteration = 0u32;
            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                unsafe {
                    let ret =
                        libusb1_sys::libusb_handle_events_timeout(ctx_ptr.as_ptr(), &mut timeval);
                    iteration += 1;
                    if iteration <= 5 || iteration % 50 == 0 {
                        log::debug!("Event loop iteration {}, ret={}", iteration, ret);
                    }
                    if ret < 0 {
                        let err = LibusbError::from(ret);
                        if err != LibusbError::Interrupted {
                            log::error!("Event loop error: {}", err);
                            break;
                        }
                    }
                }
            }

            log::info!("Event loop thread exiting after {} iterations", iteration);
        })
    };

    // Process received frames and emit to frontend
    let mut frame_count = 0u32;

    loop {
        match frame_receiver.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(frame_data) => {
                frame_count += 1;

                // Store frame in shared buffer
                {
                    let mut buffer = shared_frame_buffer.lock().unwrap();
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

/// Stream YUV 4:2:2 frames using isochronous transfers with RGB conversion
/// Supports both YUYV and UYVY formats based on streaming config
/// width/height: The negotiated resolution from UVC descriptors
/// Returns StreamResult to indicate if restart was requested
#[cfg(target_os = "android")]
fn stream_frames_yuy2(
    ctx: &LibusbContext,
    dev: &LibusbDeviceHandle,
    ep_info: &EndpointInfo,
    app_handle: AppHandle,
    shared_frame_buffer: Arc<Mutex<FrameBuffer>>,
    display_settings: Arc<Mutex<DisplaySettings>>,
    streaming_config: Arc<Mutex<StreamingConfig>>,
    _width_index: Arc<Mutex<Option<usize>>>,
    stride_index: Arc<Mutex<Option<usize>>>,
    descriptor_width: u32,
    descriptor_height: u32,
) -> Result<StreamResult, LibusbError> {
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    // Calculate expected frame size from descriptor resolution (YUY2 = 2 bytes per pixel)
    let expected_frame_size = (descriptor_width * descriptor_height * 2) as usize;

    log::info!(
        "Starting YUY2 streaming with RGB conversion, descriptor resolution: {}x{}, expected frame size: {} bytes",
        descriptor_width,
        descriptor_height,
        expected_frame_size
    );

    // Emit connected event to update frontend UI
    crate::emit_usb_event(&app_handle, true, Some("YUY2 Camera".to_string()));

    // Create the isochronous stream with descriptor-based frame size
    let mut iso_stream = unsafe {
        IsochronousStream::new(
            ctx.get_context_ptr(),
            dev.get_handle_ptr(),
            ep_info.address,
            ep_info.max_packet_size,
            expected_frame_size,
        )?
    };

    let frame_receiver = iso_stream.take_frame_receiver().ok_or(LibusbError::Other)?;
    iso_stream.start()?;

    // Spawn event loop thread
    let event_loop_handle = {
        let ctx_ptr = SendableContextPtr::new(ctx.get_context_ptr());
        let stop_flag = iso_stream.stop_flag.clone();

        std::thread::spawn(move || {
            let mut timeval = libc::timeval {
                tv_sec: 0,
                tv_usec: 100_000 as libc::suseconds_t,
            };

            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                unsafe {
                    let ret =
                        libusb1_sys::libusb_handle_events_timeout(ctx_ptr.as_ptr(), &mut timeval);
                    if ret < 0 {
                        let err = LibusbError::from(ret);
                        if err != LibusbError::Interrupted {
                            log::error!("Event loop error: {}", err);
                            break;
                        }
                    }
                }
            }
            log::info!("YUY2 event loop exiting");
        })
    };

    // Emit status update to frontend
    let _ = app_handle.emit(
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

    // Calculate expected frame size and minimum acceptable size
    // Frame may have padding, but should be at least width*height*2 bytes
    let min_expected_size = (base_width * base_height * 2) as usize;

    loop {
        // Check if restart was requested (e.g., user changed video format)
        {
            let config = streaming_config.lock().unwrap();
            if config.restart_requested {
                log::info!("Restart requested, stopping YUY2 streaming");
                iso_stream.stop();
                let _ = event_loop_handle.join();
                return Ok(StreamResult::RestartRequested);
            }
        }

        match frame_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(frame_data) => {
                frame_count += 1;
                let frame_size = frame_data.len();

                // Log detailed frame analysis for first few frames
                if frame_count <= 5 {
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

                // Get current display settings for overrides
                let settings = display_settings.lock().unwrap();
                let stride_idx = stride_index.lock().unwrap();

                // Calculate actual dimensions from frame data
                // Some cameras send more data than their descriptor claims (e.g., 1920px wide
                // despite advertising 640px). We use the actual frame size to determine the
                // real width, while trusting the descriptor height.
                let height = settings.height.unwrap_or(base_height);

                // Calculate actual stride from frame data
                let actual_stride = if frame_size > 0 && height > 0 {
                    // stride = frame_size / height, ensure even for YUY2
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
                drop(stride_idx);

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

                drop(settings); // Release lock before conversion

                // Log settings changes
                static LAST_SETTINGS: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                let settings_hash =
                    ((width as u64) << 48) | ((height as u64) << 32) | ((stride as u64) << 16);
                let last = LAST_SETTINGS.swap(settings_hash, std::sync::atomic::Ordering::Relaxed);
                if last != settings_hash {
                    log::info!("Display settings: {}x{} stride={}", width, height, stride);
                    let _ = app_handle.emit(
                        "usb-status",
                        serde_json::json!({
                            "status": "streaming",
                            "detail": format!("YUY2 {}x{} stride={} → RGB", width, height, stride)
                        }),
                    );
                };

                // Use the calculated stride as override (it's already properly computed above)
                let stride_override = Some(stride);

                // Get YUV format from streaming config
                let yuv_format = {
                    let config = streaming_config.lock().unwrap();
                    match config.yuv_format {
                        YuvFormat::Yuyv => YuvPackedFormat::Yuyv,
                        YuvFormat::Uyvy => YuvPackedFormat::Uyvy,
                    }
                };

                // Convert YUV 4:2:2 to RGB with stride
                {
                    match convert_yuv422_to_rgb(
                        &frame_data,
                        width,
                        height,
                        stride_override,
                        yuv_format,
                    ) {
                        Ok(rgb_data) => {
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

                            // Store both RGB and raw YUY2 frames in shared buffer
                            {
                                let mut buffer = shared_frame_buffer.lock().unwrap();
                                buffer.frame = rgb_data;
                                buffer.raw_frame = frame_data.clone(); // Store raw YUY2 for debugging
                                buffer.timestamp = Instant::now();
                                buffer.width = width;
                                buffer.height = height;
                            }

                            // Emit notification to trigger frontend fetch
                            let _ = app_handle.emit("frame-ready", ());

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

    log::info!("YUY2 streaming ended after {} frames", frame_count);
    Ok(StreamResult::Normal)
}

/// Start UVC streaming by sending probe/commit control requests
/// Returns the endpoint address on success.
#[cfg(target_os = "android")]
fn start_uvc_streaming(
    dev: &LibusbDeviceHandle,
    endpoint_info: Option<&EndpointInfo>,
    format_index: u8,
) -> Result<u8, LibusbError> {
    let params = start_uvc_streaming_with_resolution(dev, endpoint_info, format_index)?;
    Ok(params.endpoint)
}

/// Start UVC streaming and return full negotiated parameters including resolution.
/// Looks up width/height from the UVC frame descriptors based on negotiated frame index.
#[cfg(target_os = "android")]
fn start_uvc_streaming_with_resolution(
    dev: &LibusbDeviceHandle,
    endpoint_info: Option<&EndpointInfo>,
    format_index: u8,
) -> Result<UvcNegotiatedParams, LibusbError> {
    log::info!(
        "Initiating UVC probe/commit sequence with format_index={}",
        format_index
    );

    // Get format descriptors first so we can look up resolution
    let formats = dev.get_format_descriptors().unwrap_or_default();

    // UVC probe control - request camera format
    let mut probe = UvcStreamControl::default();
    probe.bm_hint = 1; // dwFrameInterval field is valid
    probe.b_format_index = format_index; // Try specified format
    probe.b_frame_index = 1; // First frame size

    // Request type: Class request to interface, direction OUT then IN
    let request_type_out = uvc::USB_TYPE_CLASS | uvc::USB_RECIP_INTERFACE | uvc::USB_DIR_OUT;
    let request_type_in = uvc::USB_TYPE_CLASS | uvc::USB_RECIP_INTERFACE | uvc::USB_DIR_IN;

    let streaming_interface: u16 = 1;
    let control_selector = uvc::UVC_VS_PROBE_CONTROL << 8;

    // Convert struct to bytes for transfer
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
    let mut non_jpeg_frames = 0u32;
    let mut format_confirmed = false;

    loop {
        // Perform bulk transfer to read data
        match dev.bulk_transfer(endpoint, &mut packet_buffer, timeout_ms) {
            Ok(transferred) => {
                if transferred > 0 {
                    // UVC payloads have a header (usually 12 bytes)
                    // The header contains info about frame boundaries
                    if transferred > 12 {
                        let header_len = packet_buffer[0] as usize;
                        let header_flags = packet_buffer[1];
                        let _pts = if header_len >= 6 {
                            u32::from_le_bytes([
                                packet_buffer[2],
                                packet_buffer[3],
                                packet_buffer[4],
                                packet_buffer[5],
                            ])
                        } else {
                            0
                        };

                        // Check for end of frame (bit 1 of header flags)
                        let end_of_frame = (header_flags & 0x02) != 0;

                        // Append payload data (skip header)
                        if header_len < transferred {
                            local_frame_buffer
                                .extend_from_slice(&packet_buffer[header_len..transferred]);
                        }

                        if end_of_frame && !local_frame_buffer.is_empty() {
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

                                // Store frame in shared buffer for frontend retrieval
                                {
                                    let mut buffer = shared_frame_buffer.lock().unwrap();
                                    buffer.frame = local_frame_buffer.clone();
                                    buffer.timestamp = Instant::now();
                                }

                                // Emit lightweight notification to trigger frontend fetch
                                let _ = app_handle.emit("frame-ready", ());

                                if frame_count % 30 == 0 {
                                    log::info!(
                                        "Received {} frames, last frame: {} bytes",
                                        frame_count,
                                        local_frame_buffer.len()
                                    );
                                }
                            } else {
                                non_jpeg_frames += 1;
                                log::warn!(
                                    "Non-JPEG frame received: {} bytes, header: {:02x?}",
                                    local_frame_buffer.len(),
                                    &local_frame_buffer
                                        [..std::cmp::min(16, local_frame_buffer.len())]
                                );
                            }

                            // Format detection: check after FRAMES_TO_CHECK_FORMAT frames
                            if !format_confirmed && frame_count >= FRAMES_TO_CHECK_FORMAT {
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

                            local_frame_buffer.clear();
                        }
                    }
                }
            }
            Err(LibusbError::Timeout) => {
                // Timeout is expected when no data is available
                log::trace!("Bulk transfer timeout");
            }
            Err(e) => {
                log::error!("Bulk transfer error: {}", e);
                return Err(e);
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
fn run_camera_loop(_fd: i32, _app_handle: AppHandle, _frame_buffer: Arc<Mutex<FrameBuffer>>) {
    log::info!("Camera loop not available on this platform");
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
