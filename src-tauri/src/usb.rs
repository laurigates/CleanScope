//! USB device handling for CleanScope
//!
//! This module handles USB device detection, permission management,
//! and UVC camera streaming on Android.

use tauri::AppHandle;

#[cfg(target_os = "android")]
use jni::{
    objects::{JClass, JObject, JString, JValue},
    sys::jint,
    JNIEnv,
};

/// Initialize the USB handler
/// This is called from the main thread during app setup
pub fn init_usb_handler(app_handle: AppHandle) {
    log::info!("Initializing USB handler");

    #[cfg(target_os = "android")]
    {
        // On Android, we need to get the USB file descriptor via JNI
        if let Some(fd) = get_usb_file_descriptor() {
            log::info!("USB device found with fd: {}", fd);
            crate::emit_usb_event(&app_handle, true, Some(format!("USB Camera (fd: {})", fd)));

            // Start the camera streaming loop in a new thread
            std::thread::spawn(move || {
                run_camera_loop(fd);
            });
        } else {
            log::info!("No USB device found on startup");
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        log::info!("USB handling not available on this platform");
        let _ = app_handle; // Suppress unused warning
    }
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

    // Get the device list
    let device_map = env
        .call_method(&usb_manager, "getDeviceList", "()Ljava/util/HashMap;", &[])
        .ok()?
        .l()
        .ok()?;

    // Get the values from the map
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

    // Check if there's at least one device
    let has_next = env
        .call_method(&iterator, "hasNext", "()Z", &[])
        .ok()?
        .z()
        .ok()?;

    if !has_next {
        log::info!("No USB devices found");
        return None;
    }

    // Get the first device
    let device = env
        .call_method(&iterator, "next", "()Ljava/lang/Object;", &[])
        .ok()?
        .l()
        .ok()?;

    // Check if we have permission
    let has_permission = env
        .call_method(
            &usb_manager,
            "hasPermission",
            "(Landroid/hardware/usb/UsbDevice;)Z",
            &[JValue::Object(&device)],
        )
        .ok()?
        .z()
        .ok()?;

    if !has_permission {
        log::warn!("No USB permission for device");
        return None;
    }

    log::info!("USB Permission Granted");

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

/// Run the camera streaming loop
#[cfg(target_os = "android")]
fn run_camera_loop(fd: i32) {
    log::info!("Starting camera loop with fd: {}", fd);

    // TODO: Initialize UVC context with the file descriptor
    // This is where we would use the uvc crate to:
    // 1. Create a UVC context from the file descriptor
    // 2. Open the device
    // 3. Set up frame callback
    // 4. Start streaming

    // For now, just log that we would be streaming
    log::info!("Camera loop would start streaming from fd: {}", fd);

    // Placeholder loop - in real implementation, this would be the frame processing loop
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        log::debug!("Camera loop tick (fd: {})", fd);
    }
}

#[cfg(not(target_os = "android"))]
fn run_camera_loop(_fd: i32) {
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
