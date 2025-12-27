# libusb on Android

## Overview
libusb integration on Android using file descriptor wrapping, bypassing SELinux restrictions.

## When to Use
- Accessing USB devices on Android from native code
- Working around Android's USB permission model
- Performing USB control and bulk transfers
- Implementing UVC camera streaming

## Key Concepts

### Why Direct Enumeration Fails
Android's SELinux policies prevent direct access to `/dev/bus/usb`. Instead:
1. Request USB permission through Android's UsbManager
2. Get file descriptor from UsbDeviceConnection
3. Wrap the file descriptor with libusb

### libusb1-sys with Vendored Feature
```toml
# Cargo.toml
[dependencies]
libusb1-sys = { version = "0.7", features = ["vendored"] }
rusb = "0.9"
```

The `vendored` feature compiles libusb from source, ensuring Android compatibility.

### File Descriptor Wrapping Pattern
```rust
use rusb::{Context, DeviceHandle, UsbContext};

fn wrap_android_fd(fd: i32) -> Result<DeviceHandle<Context>, rusb::Error> {
    // Create context without device discovery
    let ctx = Context::new()?;

    // Wrap the Android-provided file descriptor
    unsafe {
        let handle = ctx.wrap_sys_device(fd)?;
        Ok(handle)
    }
}
```

## Common Patterns

### Complete Android USB Setup
```rust
#[cfg(target_os = "android")]
pub fn open_usb_camera() -> Result<DeviceHandle<Context>, String> {
    // 1. Get file descriptor from JNI
    let fd = with_jni(|env, activity| {
        let usb_manager = get_usb_manager(env, activity)?;
        let devices = get_usb_devices(env, usb_manager)?;

        // Find UVC device (class 0xEF, subclass 0x02)
        for device in devices {
            if is_uvc_device(env, &device)? {
                return open_usb_device(env, usb_manager, device);
            }
        }
        Err("No UVC device found".to_string())
    })?;

    // 2. Wrap with libusb
    let handle = wrap_android_fd(fd)
        .map_err(|e| format!("Failed to wrap fd: {}", e))?;

    Ok(handle)
}
```

### Context Creation for Android
```rust
fn create_android_context() -> Result<Context, rusb::Error> {
    // Context::new() works but won't enumerate devices
    // Device discovery must come from Android's UsbManager
    Context::new()
}
```

### Interface Claiming
```rust
fn claim_video_interface(handle: &mut DeviceHandle<Context>) -> Result<(), rusb::Error> {
    // UVC devices typically have interface 1 for video streaming
    const VIDEO_STREAMING_INTERFACE: u8 = 1;

    // Detach kernel driver if attached (usually not on Android)
    let _ = handle.set_auto_detach_kernel_driver(true);

    // Claim the interface
    handle.claim_interface(VIDEO_STREAMING_INTERFACE)?;

    Ok(())
}
```

### Control Transfer
```rust
fn uvc_control_transfer(
    handle: &DeviceHandle<Context>,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    data: &mut [u8],
    timeout: Duration,
) -> Result<usize, rusb::Error> {
    handle.write_control(
        request_type,
        request,
        value,
        index,
        data,
        timeout,
    )
}
```

### Bulk Transfer for Video Data
```rust
fn read_video_bulk(
    handle: &DeviceHandle<Context>,
    endpoint: u8,
    buffer: &mut [u8],
    timeout: Duration,
) -> Result<usize, rusb::Error> {
    handle.read_bulk(endpoint, buffer, timeout)
}
```

### Finding Bulk Endpoint
```rust
fn find_bulk_endpoint(handle: &DeviceHandle<Context>) -> Result<u8, String> {
    let device = handle.device();
    let config = device.active_config_descriptor()
        .map_err(|e| e.to_string())?;

    for interface in config.interfaces() {
        for descriptor in interface.descriptors() {
            // Look for video streaming interface
            if descriptor.class_code() == 0x0E && descriptor.sub_class_code() == 0x02 {
                for endpoint in descriptor.endpoint_descriptors() {
                    if endpoint.transfer_type() == rusb::TransferType::Bulk
                        && endpoint.direction() == rusb::Direction::In
                    {
                        return Ok(endpoint.address());
                    }
                }
            }
        }
    }

    Err("No bulk endpoint found".to_string())
}
```

## Error Handling

### Common libusb Errors on Android
```rust
fn handle_usb_error(err: rusb::Error) -> String {
    match err {
        rusb::Error::Access => "Permission denied - USB permission not granted".to_string(),
        rusb::Error::NoDevice => "Device disconnected".to_string(),
        rusb::Error::Busy => "Interface already claimed".to_string(),
        rusb::Error::Timeout => "Transfer timed out".to_string(),
        rusb::Error::Pipe => "Endpoint stalled or request unsupported".to_string(),
        rusb::Error::Overflow => "Buffer overflow".to_string(),
        rusb::Error::Io => "I/O error".to_string(),
        _ => format!("USB error: {:?}", err),
    }
}
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| `LIBUSB_ERROR_ACCESS` | USB permission not granted via Android |
| `LIBUSB_ERROR_NO_DEVICE` | File descriptor invalid or device disconnected |
| `LIBUSB_ERROR_BUSY` | Call `release_interface` before `claim_interface` |
| `LIBUSB_ERROR_NOT_SUPPORTED` | Feature not supported (try different approach) |
| Build error with `vendored` | Check NDK toolchain paths |

## Build Configuration

### Android.mk Additions (if needed)
```makefile
LOCAL_LDLIBS += -llog -landroid
```

### CMakeLists.txt (if using CMake)
```cmake
find_library(log-lib log)
target_link_libraries(${TARGET} ${log-lib})
```

## References
- [rusb Documentation](https://docs.rs/rusb/)
- [libusb1-sys](https://docs.rs/libusb1-sys/)
- [Android USB Host](https://developer.android.com/guide/topics/connectivity/usb/host)
