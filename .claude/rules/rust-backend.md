---
paths:
  - "src-tauri/**/*.rs"
---

# Rust Backend Rules

## Overview

Rules for Rust code in the Tauri backend. Applies to all `.rs` files in `src-tauri/`.

## Platform Conditionals

Use `#[cfg(target_os = "android")]` for Android-specific code:

```rust
#[cfg(target_os = "android")]
mod libusb_android;

#[cfg(target_os = "android")]
fn android_only_function() { ... }

#[cfg(not(target_os = "android"))]
fn desktop_stub() { ... }
```

Desktop builds must compile cleanly with stubbed functionality.

## Error Handling

Use the unified `AppError` type for Tauri commands:

```rust
use crate::AppError;

#[tauri::command]
fn my_command() -> Result<T, AppError> {
    // Use ? operator to propagate errors
    let value = fallible_operation()?;
    Ok(value)
}
```

Avoid `unwrap()` and `expect()` in production paths. Use proper error propagation.

## Unsafe Code

Minimize unsafe blocks. When required (FFI, libusb):

1. Document why unsafe is necessary
2. Keep unsafe blocks as small as possible
3. Validate all inputs before unsafe operations
4. Use safe wrappers where possible

```rust
// SAFETY: fd is a valid file descriptor from Android UsbDeviceConnection
unsafe {
    libusb_wrap_sys_device(ctx, fd, &mut handle)
}
```

## Concurrency Patterns

Use `Arc<Mutex<T>>` for shared state:

```rust
pub struct AppState {
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
    pub display_settings: Arc<Mutex<DisplaySettings>>,
}
```

Avoid holding locks across async boundaries. Clone Arc handles for long-lived operations.

## Logging

Use structured logging with appropriate levels:

```rust
log::info!("Starting USB stream");
log::debug!("Frame received: {} bytes", frame.len());
log::warn!("Frame validation warning: {}", msg);
log::error!("USB error: {}", e);
```

For Android, logs appear via `adb logcat -s RustStdoutStderr:*`.

## JNI Callbacks

JNI callback functions must:
1. Use `#[no_mangle]` and `extern "C"`
2. Follow exact Java naming convention
3. Handle null pointers safely
4. Not panic (use catch_unwind if needed)

```rust
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_cleanscope_app_MainActivity_onUsbPermissionResult(
    env: JNIEnv,
    _class: JClass,
    granted: jboolean,
) {
    // Implementation
}
```
