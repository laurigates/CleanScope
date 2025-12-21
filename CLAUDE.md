# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CleanScope is a privacy-respecting Android application for viewing video feeds from USB-C endoscopes (UVC devices). Built with Tauri v2 + Rust to avoid Java/Kotlin complexity.

## Development Commands

```bash
# Install dependencies
npm install

# Desktop development
npm run tauri:dev

# Android development (requires SDK + NDK)
npm run tauri:android:dev

# Build Android APK
npm run tauri:android:build

# Type checking (Svelte/TypeScript)
npm run check

# Rust checks (from src-tauri/)
cargo check
cargo clippy

# View Android logs (filter by app tag)
adb logcat -s CleanScope:* RustStdoutStderr:*
```

## Architecture

**Frontend (src/):** Svelte 5 with runes syntax. Communicates with Rust via Tauri commands (`invoke()`) and event listeners (`listen()`).

**Backend (src-tauri/):**
- `lib.rs` - Tauri commands, app setup, event emission
- `usb.rs` - USB device handling via JNI (Android-only, `#[cfg(target_os = "android")]`)

**Android USB Flow:**
1. USB camera plugged in → Android triggers `USB_DEVICE_ATTACHED` intent
2. App auto-launches via AndroidManifest.xml intent filter
3. Permission auto-granted via device_filter.xml matching
4. `usb.rs` gets file descriptor via JNI → UsbManager → UsbDeviceConnection
5. Frames streamed and emitted to frontend via `emit_camera_frame()`

## Key Modification Points

| Change | File |
|--------|------|
| Add Tauri command | `src-tauri/src/lib.rs` - add `#[tauri::command]` fn, register in `invoke_handler` |
| Add USB device support | `src-tauri/gen/android/.../res/xml/device_filter.xml` - add vendor/product IDs |
| UI changes | `src/App.svelte` |
| JNI callbacks | `src-tauri/src/usb.rs` - `Java_com_cleanscope_app_MainActivity_*` functions |

## Platform Considerations

- Desktop builds compile but USB functionality is stubbed (Android-only via JNI)
- Android builds require: Rust target `aarch64-linux-android`, Android SDK, NDK (`NDK_HOME` env var)
- Most endoscopes use MJPEG format (handled by `jpeg-decoder` crate)
- UVC library commented out in Cargo.toml pending vendored libusb fork for Android
