# CleanScope - Development Guide

## Project Overview

CleanScope is a privacy-respecting Android application for viewing video feeds from USB-C endoscopes (UVC devices). Built with Tauri v2 + Rust to avoid Java/Kotlin complexity.

## Architecture

```
CleanScope/
├── src/                    # Svelte frontend (UI overlay)
│   ├── App.svelte          # Main application component
│   └── main.ts             # Entry point
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── lib.rs          # Tauri commands and app setup
│   │   ├── main.rs         # Desktop entry point
│   │   └── usb.rs          # USB/UVC handling via JNI
│   ├── gen/android/        # Android-specific configuration
│   │   └── app/src/main/
│   │       ├── AndroidManifest.xml  # USB intent filters
│   │       └── res/xml/device_filter.xml  # USB device matching
│   ├── Cargo.toml          # Rust dependencies
│   ├── build.rs            # Build script for Android linking
│   └── tauri.conf.json     # Tauri configuration
└── package.json            # Frontend dependencies
```

## Key Technologies

- **Tauri v2**: App shell, handles APK bundling and WebView
- **Rust**: Core logic, USB handling, performance-critical code
- **Svelte 5**: Lightweight UI layer with runes syntax
- **JNI**: Bridge to Android APIs for USB file descriptors
- **UVC**: USB Video Class protocol for camera communication

## Development Commands

```bash
# Install dependencies
npm install

# Development (desktop)
npm run tauri:dev

# Development (Android) - requires Android SDK
npm run tauri:android:dev

# Build for Android
npm run tauri:android:build

# Type checking
npm run check
```

## Android USB Flow

1. User plugs in USB camera
2. Android triggers `USB_DEVICE_ATTACHED` intent
3. App auto-launches (configured in AndroidManifest.xml)
4. Permission granted automatically via intent filter
5. Rust code gets file descriptor via JNI
6. UVC library streams frames to native surface

## Key Files to Modify

### Adding Tauri Commands
Edit `src-tauri/src/lib.rs` to add new `#[tauri::command]` functions.

### USB Device Support
Edit `src-tauri/gen/android/app/src/main/res/xml/device_filter.xml` to add specific vendor/product IDs.

### UI Changes
Edit `src/App.svelte` for frontend modifications.

## Dependencies Notes

- **jni**: Rust JNI bindings for Android API access
- **ndk**: Android NDK bindings for native functionality
- **android_logger**: Logging that appears in logcat
- **jpeg-decoder**: Pure Rust MJPEG decoding (most endoscopes use MJPEG)

## Build Requirements

- Rust with `aarch64-linux-android` target
- Node.js 18+
- Android SDK (for Android builds)
- Android NDK (set `NDK_HOME` environment variable)

## Testing on Device

1. Enable USB Debugging on Android device
2. Connect device via USB
3. Run `npm run tauri:android:dev`
4. Plug in endoscope to device
5. Check logcat for "CleanScope" tag logs
