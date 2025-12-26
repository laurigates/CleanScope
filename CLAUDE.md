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

### Setup Procedure

1. **Connect phone via USB** (temporarily, for initial setup)

2. **Enable TCP/IP mode:**
   ```bash
   adb tcpip 5555
   ```

3. **Get phone's IP address:**
   ```bash
   adb shell ip addr show wlan0 | grep "inet "
   ```
   Look for the IP address (e.g., `192.168.1.42`)

4. **Connect wirelessly:**
   ```bash
   adb connect <PHONE_IP>:5555
   ```

5. **Unplug USB cable** - ADB now runs over WiFi

6. **Deploy the app:**
   ```bash
   just android-dev
   ```

7. **Plug endoscope into phone's USB-C port**

8. **Monitor logs:**
   ```bash
   just logs
   ```

### Requirements

- Phone and computer must be on the same network
- Vite is configured with `host: "0.0.0.0"` in `vite.config.ts` for network accessibility

### Reconnection

- Connection persists until phone reboots
- If connection drops, reconnect with: `adb connect <PHONE_IP>:5555`
- Verify connection: `adb devices`

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
- Most endoscopes use MJPEG format (handled by `jpeg-decoder` crate)
- UVC library commented out in Cargo.toml pending vendored libusb fork for Android
