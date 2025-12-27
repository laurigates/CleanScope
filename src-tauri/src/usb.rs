//! USB device handling for `CleanScope`
//!
//! This module handles USB device detection, permission management,
//! and UVC camera streaming on Android.

use std::sync::{Arc, Mutex};
use tauri::AppHandle;

#[cfg(target_os = "android")]
use tauri::Emitter;

use crate::FrameBuffer;

#[cfg(target_os = "android")]
use jni::{
    objects::{JClass, JObject, JValue},
    sys::jint,
    JNIEnv,
};

#[cfg(target_os = "android")]
use crate::libusb_android::{uvc, LibusbContext, LibusbDeviceHandle, LibusbError};

/// Initialize the USB handler
/// This is called from the main thread during app setup
pub fn init_usb_handler(app_handle: AppHandle, frame_buffer: Arc<Mutex<FrameBuffer>>) {
    log::info!("Initializing USB handler");

    #[cfg(target_os = "android")]
    {
        // On Android, we need to get the USB file descriptor via JNI
        if let Some(fd) = get_usb_file_descriptor() {
            log::info!("USB device found with fd: {}", fd);
            crate::emit_usb_event(&app_handle, true, Some(format!("USB Camera (fd: {})", fd)));

            // Start the camera streaming loop in a new thread
            std::thread::spawn(move || {
                run_camera_loop(fd, app_handle, frame_buffer);
            });
        } else {
            log::info!("No USB device found on startup");
        }
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

/// Run the camera streaming loop
#[cfg(target_os = "android")]
fn run_camera_loop(fd: i32, app_handle: AppHandle, frame_buffer: Arc<Mutex<FrameBuffer>>) {
    log::info!("Starting camera loop with fd: {}", fd);

    match run_camera_loop_inner(fd, app_handle, frame_buffer) {
        Ok(()) => log::info!("Camera loop ended normally"),
        Err(e) => log::error!("Camera loop error: {}", e),
    }
}

#[cfg(target_os = "android")]
fn run_camera_loop_inner(
    fd: i32,
    app_handle: AppHandle,
    frame_buffer: Arc<Mutex<FrameBuffer>>,
) -> Result<(), LibusbError> {
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

    // Claim the video streaming interface (typically interface 1)
    // Interface 0 is usually the control interface, interface 1 is streaming
    let streaming_interface = 1;
    if let Err(e) = dev.claim_interface(streaming_interface) {
        log::warn!("Could not claim interface {}: {}", streaming_interface, e);
        // Try interface 0 as fallback
        dev.claim_interface(0)?;
    }

    // Start UVC streaming
    match start_uvc_streaming(&dev) {
        Ok(endpoint) => {
            log::info!("UVC streaming started on endpoint 0x{:02x}", endpoint);
            stream_frames(&dev, endpoint, app_handle, frame_buffer)?;
        }
        Err(e) => {
            log::error!("Failed to start UVC streaming: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// Start UVC streaming by sending probe/commit control requests
#[cfg(target_os = "android")]
fn start_uvc_streaming(dev: &LibusbDeviceHandle) -> Result<u8, LibusbError> {
    log::info!("Initiating UVC probe/commit sequence");

    // UVC probe control - request the camera's default format
    let mut probe = UvcStreamControl::default();
    probe.bm_hint = 1; // dwFrameInterval field is valid
    probe.b_format_index = 1; // First format (usually MJPEG)
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
    let format_index = negotiated.b_format_index;
    let frame_index = negotiated.b_frame_index;
    let max_frame_size = negotiated.dw_max_video_frame_size;
    log::info!(
        "Negotiated: format={} frame={} max_frame_size={}",
        format_index,
        frame_index,
        max_frame_size
    );

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

    // Return the streaming endpoint address (typically 0x81 for bulk IN)
    // This should be read from the endpoint descriptor, but most USB cameras use 0x81
    Ok(0x81)
}

/// Stream frames from the camera
#[cfg(target_os = "android")]
fn stream_frames(
    dev: &LibusbDeviceHandle,
    endpoint: u8,
    app_handle: AppHandle,
    shared_frame_buffer: Arc<Mutex<FrameBuffer>>,
) -> Result<(), LibusbError> {
    use std::time::Instant;

    log::info!("Starting frame streaming from endpoint 0x{:02x}", endpoint);

    // Buffer for receiving USB data
    // USB packets are typically up to 512 bytes (full-speed) or 1024 bytes (high-speed)
    // MJPEG frames can be several KB, so we need to accumulate packets
    let mut packet_buffer = vec![0u8; 16384]; // 16KB per transfer
    let mut local_frame_buffer = Vec::with_capacity(1024 * 1024); // 1MB for frame accumulation

    let timeout_ms = 1000;
    let mut frame_count = 0u32;

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
                            if local_frame_buffer.len() >= 2
                                && local_frame_buffer[0] == 0xFF
                                && local_frame_buffer[1] == 0xD8
                            {
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
                                    // Note: width/height would need JPEG parsing to determine
                                    // For now, leave as 0 (frontend uses actual decoded dimensions)
                                }

                                // Emit lightweight notification (no payload) to trigger frontend fetch
                                let _ = app_handle.emit("frame-ready", ());

                                if frame_count % 30 == 0 {
                                    log::info!(
                                        "Received {} frames, last frame: {} bytes",
                                        frame_count,
                                        local_frame_buffer.len()
                                    );
                                }
                            } else {
                                log::warn!(
                                    "Non-JPEG frame received: {} bytes, header: {:02x?}",
                                    local_frame_buffer.len(),
                                    &local_frame_buffer
                                        [..std::cmp::min(16, local_frame_buffer.len())]
                                );
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
