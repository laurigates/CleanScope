# CleanScope

Privacy-respecting USB endoscope viewer for Android. No cloud, no telemetry, no complex Java/Kotlin — just pure Rust and Svelte running locally on your device.

CleanScope lets you view video feeds from USB-C endoscopes (UVC devices) on Android, with full control over your data and complete transparency over what the app does with your camera.

## Features

- **Privacy-First Design** - All video processing happens locally on your device. No cloud uploads, no telemetry, no data collection.
- **USB-C Endoscope Support** - Works with any standard UVC (USB Video Class) endoscope via USB-C.
- **Auto-Launch on Connection** - App automatically launches when you plug in an endoscope. Permission is auto-granted via device filter.
- **Format Flexibility** - Supports both MJPEG (compressed) and YUY2/YUYV (uncompressed) video formats. Toggle between formats and resolutions via the UI.
- **Frame Validation** - Built-in corruption detection helps debug camera issues (adjustable sensitivity via environment variables).
- **ADB over WiFi Support** - Stream and control the app over WiFi while the USB-C port remains free for the endoscope.

## Tech Stack

| Category | Technology |
|----------|-----------|
| Runtime | Rust + Tauri v2 (Android) |
| Frontend | Svelte 5 with runes syntax |
| Language | TypeScript, Rust |
| USB Handling | Custom UVC negotiation + isochronous transfers via Android JNI |
| Video Processing | YUV→RGB conversion, frame assembly, validation |
| CI/CD | GitHub Actions (Rust tests, lint, typecheck, E2E tests) |

## Prerequisites

Before you can build and run CleanScope:

- **Rust** - Latest stable (install from [rustup.rs](https://rustup.rs/))
- **Rust Android targets** - Run `rustup target add aarch64-linux-android`
- **Android SDK** - API level 34 or higher
- **Android NDK** - Version 26+ (set `NDK_HOME` environment variable)
- **Node.js** - Version 20+
- **ADB** - Android Debug Bridge (comes with SDK platform-tools)
- **Tauri CLI** - Installed via npm

**Quick setup (macOS):**
```bash
# Install prerequisites using the justfile
just setup

# Verify everything is installed
just check-prereqs
```

## Getting Started

### Installation

```bash
# Clone the repository
git clone https://github.com/laurigates/CleanScope.git
cd CleanScope

# Install npm dependencies
just install

# Verify prerequisites
just check-prereqs
```

### Desktop Development

Desktop builds compile but USB functionality is stubbed (Android-only). Useful for testing UI changes:

```bash
# Start desktop development server
just dev
```

### Android Development

#### Physical Device

```bash
# Connect phone via USB and build/deploy
just android-dev

# Or: build standalone APK
just android-build
```

#### Emulator

Note: USB OTG is not supported in emulators. Use a physical device for endoscope testing.

```bash
# One-time setup
just setup-emulator
just emulator-create

# In terminal 1: start emulator (keep running)
just emulator-run

# In terminal 2: deploy app
just android-dev
```

### USB Endoscope Testing with ADB over WiFi

Most Android phones have a single USB-C port. Use ADB over WiFi to deploy and debug while the endoscope is connected:

```bash
# Step 1: Connect phone via USB, run setup
just wifi-setup

# Step 2: Unplug USB cable

# Step 3: Plug endoscope into USB-C port

# Step 4: Deploy and watch logs
just endoscope-test
```

For reconnection without USB (if WiFi pairing persists):
```bash
just wifi-connect 192.168.0.25  # Replace with your phone's IP
```

## Development Commands

Run `just --list` to see all recipes. Key commands:

```bash
# Building & Deployment
just dev                 # Desktop development
just android-dev        # Build and deploy to Android device
just android-build      # Build Android APK
just wifi-deploy        # Build and deploy over WiFi

# Testing & Quality
just check-all          # Run all checks (lint, format, typecheck, rust)
just lint               # Check code style (Biome)
just lint-fix           # Fix style issues
just typecheck          # TypeScript/Svelte type check
npm run test:e2e        # Run Playwright E2E tests
just rust-check         # Rust cargo check
just rust-clippy        # Rust linter

# Debugging
just logs               # Stream app logs from device
just usb-debug          # Show connected USB devices and logs
just force-stop         # Stop the running app
just launch             # Launch app without rebuilding
```

## Architecture

CleanScope is built with a clean separation between frontend and backend:

**Frontend** (`src/`): Svelte 5 with reactive runes syntax. Communicates with Rust backend via Tauri commands (`invoke()`) and event listeners (`listen()`).

**Backend** (`src-tauri/`):
- `lib.rs` - Tauri commands, app initialization, event emission, display settings
- `usb.rs` - UVC negotiation, format detection, YUV→RGB conversion, frame streaming
- `libusb_android.rs` - Low-level USB/isochronous transfers, UVC header parsing, frame assembly
- `frame_validation.rs` - Frame corruption detection (row similarity, size, stride checks)

### Android USB Flow

1. USB endoscope plugged in → Android triggers `USB_DEVICE_ATTACHED` intent
2. App auto-launches via AndroidManifest.xml intent filter
3. Permission auto-granted via device_filter.xml USB device matching
4. Rust backend gets file descriptor via JNI → UsbManager → UsbDeviceConnection
5. UVC PROBE/COMMIT negotiates video format (MJPEG or YUY2) and resolution
6. Isochronous USB transfers stream video data
7. Frames are assembled, validated, converted to RGB, emitted to frontend
8. Frontend renders frames on canvas at 30+ FPS

### Video Pipeline

```
Endoscope → USB Transfer → Frame Assembly → Validation → YUV→RGB → Canvas
```

**For technical depth**, see:
- [`docs/VIDEO_PIPELINE.md`](docs/VIDEO_PIPELINE.md) - Complete video processing documentation
- [`docs/blueprint/feature-tracker.json`](docs/blueprint/feature-tracker.json) - Feature status and progress
- [`docs/adrs/`](docs/adrs/) - Architecture Decision Records

## Testing

### Frontend Tests

```bash
# Run E2E tests with Playwright
npm run test:e2e

# Run with browser visible for debugging
npm run test:e2e:headed
```

### Rust Tests

Rust unit and integration tests compile and run on desktop. Android tests require additional setup:

```bash
# Run Rust tests (desktop)
cd src-tauri && cargo test

# Type-check Rust code
just rust-check

# Run clippy linter
just rust-clippy
```

### Integration Testing

Use the endoscope testing workflow to validate end-to-end behavior:

```bash
just endoscope-test
```

This builds, deploys, launches the app, and streams logs while you test with an actual endoscope.

## Environment Variables

### CLEANSCOPE_FRAME_VALIDATION

Controls frame corruption detection strictness:

| Value | Behavior |
|-------|----------|
| `strict` (default) | Row similarity + size + alignment checks. Detects banding and shearing. |
| `moderate` | Size checks only (10% tolerance) |
| `minimal` | Only massive size mismatches (2x tolerance) |
| `off` | No validation, best performance |

Usage:
```bash
CLEANSCOPE_FRAME_VALIDATION=moderate just android-dev
```

## License

This project is licensed under the [MIT License](LICENSE).

## Learn More

- **Development Setup**: See [`CLAUDE.md`](CLAUDE.md) for detailed development workflow, conventions, and debugging tips.
- **Architecture**: See [`docs/blueprint/`](docs/blueprint/) for feature tracking, work orders, and architectural decisions.
- **Video Processing**: See [`docs/VIDEO_PIPELINE.md`](docs/VIDEO_PIPELINE.md) for frame assembly, format negotiation, and troubleshooting.
