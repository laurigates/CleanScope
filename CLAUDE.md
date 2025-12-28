# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CleanScope is a privacy-respecting Android application for viewing video feeds from USB-C endoscopes (UVC devices). Built with Tauri v2 + Rust to avoid Java/Kotlin complexity.

## Development Commands

Run `just --list` for all available recipes. Key commands:

```bash
# Install dependencies
just install

# Desktop development
just dev

# Android development (physical device)
just android-dev

# Android development (emulator)
# Terminal 1: Start emulator (keep running)
just emulator-run
# Terminal 2: Deploy app (after emulator boots)
just android-dev

# Build Android APK
just android-build

# Run all checks (lint, format, typecheck, rust)
just check-all

# View Android logs
just logs
```

### Emulator Setup (one-time)

```bash
just setup-emulator    # Install emulator + system image
just emulator-create   # Create AVD named "cleanscope"
```

**Note:** USB OTG is not supported in emulators. Use a physical device for endoscope testing.

## ADB over WiFi (USB Endoscope Testing)

Most Android phones have a single USB-C port, which is needed for the endoscope. Use ADB over WiFi to deploy and debug while the endoscope is connected.

### Quick Start (Justfile Shortcuts)

```bash
just wifi-setup      # Automated: enable tcpip, detect IP, connect
just wifi-status     # Check connection type (WiFi vs USB)
just wifi-deploy     # Build + install + launch in one command
just endoscope-test  # Full workflow: deploy + stream logs
```

### Manual Setup Procedure

1. **Connect phone via USB** (temporarily, for initial setup)

2. **Run automated setup:**
   ```bash
   just wifi-setup
   ```
   This enables TCP/IP mode, detects the phone's IP, and connects automatically.

3. **Unplug USB cable** - ADB now runs over WiFi

4. **Deploy and test:**
   ```bash
   just endoscope-test
   ```
   This builds, installs, launches the app, and streams logs.

5. **Plug endoscope into phone's USB-C port**

### Requirements

- Phone and computer must be on the same network
- Vite is configured with `host: "0.0.0.0"` in `vite.config.ts` for network accessibility

### Reconnection

- Connection persists until phone reboots
- Check status: `just wifi-status`
- If connection drops, run: `just wifi-setup` again

### Alternative: Native Wireless Debugging (Android 11+)

Android 11 and later have built-in wireless debugging:

1. Go to **Settings > Developer Options > Wireless debugging**
2. Enable and tap to view pairing code and port
3. Pair: `adb pair <IP>:<PAIRING_PORT>` (enter pairing code when prompted)
4. Connect: `adb connect <IP>:<DEBUG_PORT>`

This method survives reboots after initial pairing.

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
- UVC library commented out in Cargo.toml pending vendored libusb fork for Android

## Video Format Notes

**UVC Format Negotiation:**
- Endoscopes advertise multiple formats (MJPEG, YUY2/uncompressed)
- Format index 1 is NOT guaranteed to be MJPEG - varies by device
- Current code hardcodes `b_format_index = 1` in `usb.rs:start_uvc_streaming()`
- If frames lack JPEG markers (0xFFD8), try incrementing format_index

**Format Detection:**
- MJPEG frames: Start with 0xFFD8, typically 5-50KB
- YUY2 frames: No markers, size = width × height × 2 bytes
- Frame size hints: 50-125KB without JPEG markers likely indicates YUY2
