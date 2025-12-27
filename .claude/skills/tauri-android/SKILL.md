# Tauri Android Development

## Overview
Patterns and troubleshooting for Tauri v2 Android development, including environment setup, cross-compilation, and platform-specific code.

## When to Use
- Setting up Android development environment
- Debugging Android build failures
- Writing platform-specific Rust code
- Configuring Android manifest and permissions
- Troubleshooting JNI and native code issues

## Key Concepts

### Environment Setup
Required environment variables:
```bash
export ANDROID_HOME="/path/to/android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/<version>"
export PATH="$ANDROID_HOME/platform-tools:$PATH"
```

### Rust Cross-Compilation Targets
```bash
# Install Android targets
rustup target add aarch64-linux-android   # ARM64 (most devices)
rustup target add armv7-linux-androideabi  # ARM32 (older devices)
rustup target add i686-linux-android       # x86 (emulator)
rustup target add x86_64-linux-android     # x86_64 (emulator)
```

### Platform-Specific Code
Use cfg attributes for Android-only code:
```rust
#[cfg(target_os = "android")]
fn android_only_function() {
    // Android-specific implementation
}

#[cfg(not(target_os = "android"))]
fn android_only_function() {
    // Stub for other platforms
}
```

### Android Logger Initialization
```rust
#[cfg(target_os = "android")]
fn init_logging() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("CleanScope"),
    );
}
```

## Common Patterns

### Tauri Command with Android-Specific Behavior
```rust
#[tauri::command]
fn get_device_info() -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        // Use JNI to get Android device info
        Ok("Android device".to_string())
    }

    #[cfg(not(target_os = "android"))]
    {
        Ok("Desktop".to_string())
    }
}
```

### Event Emission from Native Code
```rust
use tauri::Emitter;

fn emit_event(app: &tauri::AppHandle, payload: &str) {
    app.emit("event-name", payload)
        .expect("Failed to emit event");
}
```

### Binary Data Response (No Base64)
```rust
use tauri::ipc::Response;

#[tauri::command]
fn get_frame() -> Response {
    let frame_data: Vec<u8> = get_frame_bytes();
    Response::new(frame_data)
}
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| `NDK_HOME not set` | Set NDK_HOME to `$ANDROID_HOME/ndk/<version>` |
| `aarch64-linux-android-clang not found` | NDK path incorrect or NDK not installed |
| `cannot find -llog` | Add `log` to Android.mk or CMakeLists.txt |
| `UnsatisfiedLinkError` at runtime | JNI function signature mismatch |
| `INSTALL_FAILED_UPDATE_INCOMPATIBLE` | Uninstall existing app: `adb uninstall com.cleanscope.app` |
| Build fails with "SDK not found" | Run `just check-prereqs` and fix missing components |

## Build Commands
```bash
# Check environment
just check-prereqs

# Debug build
just android-build

# Release build
just android-release

# Install to device
just android-install

# Full development cycle
just android-dev
```

## References
- [Tauri v2 Mobile Docs](https://v2.tauri.app/develop/mobile/)
- [Android NDK Docs](https://developer.android.com/ndk/guides)
- [Rust Android Target](https://rust-lang.github.io/rustup/cross-compilation.html)
