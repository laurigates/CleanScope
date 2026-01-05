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
just wifi-setup      # Automated: enable tcpip, detect IP, connect (requires USB)
just wifi-connect    # Reconnect to known IP without USB (default: 192.168.0.25)
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
- If connection drops: `just wifi-connect` (no USB needed if IP unchanged)
- For new IP or first-time setup: `just wifi-setup` (requires USB)

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
- `lib.rs` - Tauri commands, app setup, event emission, display settings
- `usb.rs` - UVC negotiation, YUV conversion, frame streaming
- `libusb_android.rs` - Low-level USB/isochronous transfers, UVC header parsing, frame assembly

**Android USB Flow:**
1. USB camera plugged in → Android triggers `USB_DEVICE_ATTACHED` intent
2. App auto-launches via AndroidManifest.xml intent filter
3. Permission auto-granted via device_filter.xml matching
4. `usb.rs` gets file descriptor via JNI → UsbManager → UsbDeviceConnection
5. UVC PROBE/COMMIT negotiates format (MJPEG or YUY2) and resolution
6. Isochronous transfers stream video data via `libusb_android.rs`
7. Frames assembled, converted to RGB, emitted to frontend

## Video Pipeline

```
USB Camera (UVC) → Isochronous USB Transfers → Frame Assembly → YUV→RGB → Frontend Canvas
     ↓                      ↓                        ↓              ↓           ↓
 Advertises            libusb_android.rs      process_iso_packets  usb.rs    App.svelte
 640x480 YUY2          (1024-byte packets)    (strip headers,     (yuvutils)  (ImageData)
                                               accumulate)
```

**Key files in pipeline:**
| Stage | File | Function |
|-------|------|----------|
| UVC negotiation | `usb.rs` | `start_uvc_streaming()` |
| Isochronous transfers | `libusb_android.rs` | `IsochronousStream`, `iso_transfer_callback` |
| UVC header parsing | `libusb_android.rs` | `validate_uvc_header()` |
| Frame assembly | `libusb_android.rs` | `process_iso_packets()` |
| YUV→RGB conversion | `usb.rs` | `convert_yuv422_to_rgb()`, `stream_frames_yuy2()` |
| Display | `src/App.svelte` | `renderFrame()` |

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
- App has UI toggle to skip MJPEG detection and go straight to YUY2
- If frames lack JPEG markers (0xFFD8), app falls back to YUY2 processing

**Format Detection:**
- MJPEG frames: Start with 0xFFD8, typically 5-50KB
- YUY2 frames: No markers, size = width × height × 2 bytes (e.g., 614,400 for 640×480)
- Frame size hints: 50-125KB without JPEG markers likely indicates YUY2

**YUV Pixel Formats:**
- YUYV (YUY2): Byte order Y0-U-Y1-V (luminance first) - most common
- UYVY: Byte order U-Y0-V-Y1 (chrominance first) - less common
- App has UI toggle to switch between YUYV and UYVY
- Wrong format causes green/magenta color cast

## Debugging Video Issues

**Common artifacts and causes:**

| Artifact | Likely Cause | Fix |
|----------|--------------|-----|
| Horizontal stripes/interlacing | Frame boundary misalignment | Check `process_iso_packets` buffer handling |
| Diagonal stripes/shearing | Stride mismatch | Adjust stride via UI or check `actual_stride` calculation |
| Green/magenta tint | Wrong YUV format (YUYV vs UYVY) | Toggle YUV format in UI |
| Shifted/offset rows | UVC headers treated as pixel data | Check `validate_uvc_header()` |
| Black frames | Format detection failed | Check MJPEG skip toggle, verify frame size |

**UVC Header Validation:**
- Headers are 2-12 bytes with EOH bit (0x80) in byte 1
- Cheap cameras may not follow spec strictly (reserved bits set, length mismatches)
- `validate_uvc_header()` uses relaxed validation - trusts length if EOH is set
- If headers are rejected, their bytes get added as pixel data → corruption

**Debug UI controls:**
- Width/Height/Stride/Offset buttons: Override auto-detected values
- MJPEG toggle: Skip MJPEG detection, force YUY2
- YUV toggle: Switch between YUYV and UYVY byte order
- Capture button: Save raw and RGB frames for offline analysis

**Useful log patterns:**
```bash
# Watch frame assembly
adb logcat -s RustStdoutStderr:* | grep -E "frame|Frame|YUY2|MJPEG|header"

# Check for suspicious packets (headers treated as data)
adb logcat -s RustStdoutStderr:* | grep "SUSPICIOUS"

# Watch stride/resolution detection
adb logcat -s RustStdoutStderr:* | grep -E "stride|resolution|width|height"
```
