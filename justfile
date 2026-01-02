# CleanScope Development Justfile
# Run `just` or `just --list` to see available recipes

set shell := ["bash", "-uc"]

# Default recipe - show help
default:
    @just --list

# ============================================================================
# PREREQUISITES & SETUP
# ============================================================================

# Check all prerequisites for Android development
check-prereqs:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Checking Android development prerequisites..."
    echo ""

    errors=0

    # Check Node.js
    if command -v node &>/dev/null; then
        echo "✓ Node.js: $(node --version)"
    else
        echo "✗ Node.js: not found"
        ((errors++))
    fi

    # Check Rust
    if command -v rustc &>/dev/null; then
        echo "✓ Rust: $(rustc --version | cut -d' ' -f2)"
    else
        echo "✗ Rust: not found"
        ((errors++))
    fi

    # Check Rust Android targets
    if rustup target list --installed | grep -q "aarch64-linux-android"; then
        echo "✓ Rust target: aarch64-linux-android"
    else
        echo "✗ Rust target: aarch64-linux-android not installed"
        ((errors++))
    fi

    # Check Android SDK
    if [[ -n "${ANDROID_HOME:-}" ]] && [[ -d "$ANDROID_HOME" ]]; then
        echo "✓ ANDROID_HOME: $ANDROID_HOME"
    else
        echo "✗ ANDROID_HOME: not set or directory doesn't exist"
        ((errors++))
    fi

    # Check NDK
    if [[ -n "${NDK_HOME:-}" ]] && [[ -d "$NDK_HOME" ]]; then
        echo "✓ NDK_HOME: $NDK_HOME"
    else
        echo "✗ NDK_HOME: not set or directory doesn't exist"
        ((errors++))
    fi

    # Check ADB
    if command -v adb &>/dev/null; then
        echo "✓ ADB: $(which adb)"
    else
        echo "✗ ADB: not found"
        ((errors++))
    fi

    # Check Tauri CLI
    if command -v cargo-tauri &>/dev/null || npm list @tauri-apps/cli &>/dev/null 2>&1; then
        echo "✓ Tauri CLI: available"
    else
        echo "✗ Tauri CLI: not found"
        ((errors++))
    fi

    echo ""
    if [[ $errors -eq 0 ]]; then
        echo "All prerequisites satisfied!"
    else
        echo "Missing $errors prerequisite(s). Run 'just setup' to install."
        exit 1
    fi

# Install all prerequisites (macOS)
setup: setup-android-sdk setup-rust-targets
    @echo ""
    @echo "Setup complete! Restart your shell or run:"
    @echo "  source ~/.zshrc  # or your shell config"
    @echo ""
    @echo "Then run 'just check-prereqs' to verify."

# Install Android SDK and NDK via Homebrew
setup-android-sdk:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Setting up Android SDK..."

    # Install command-line tools if not present
    if ! brew list android-commandlinetools &>/dev/null 2>&1; then
        echo "Installing Android command-line tools..."
        brew install --cask android-commandlinetools
    else
        echo "Android command-line tools already installed"
    fi

    # Homebrew installs SDK to this location
    SDK_ROOT="/opt/homebrew/share/android-commandlinetools"

    # Accept licenses
    yes | sdkmanager --licenses || true

    # Install required components
    echo "Installing SDK components..."
    sdkmanager "platform-tools" \
               "platforms;android-34" \
               "build-tools;34.0.0" \
               "ndk;26.1.10909125"

    # Get NDK version installed
    NDK_VERSION=$(ls "$SDK_ROOT/ndk" 2>/dev/null | head -1 || echo "26.1.10909125")

    echo ""
    echo "Add these to your shell configuration (~/.zshrc):"
    echo ""
    echo "export ANDROID_HOME=\"$SDK_ROOT\""
    echo "export NDK_HOME=\"\$ANDROID_HOME/ndk/$NDK_VERSION\""
    echo "export PATH=\"\$ANDROID_HOME/platform-tools:\$PATH\""

# Install Rust Android targets
setup-rust-targets:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Installing Rust Android targets..."
    rustup target add aarch64-linux-android
    rustup target add armv7-linux-androideabi
    rustup target add i686-linux-android
    rustup target add x86_64-linux-android
    echo "Rust Android targets installed."

# Install Android emulator and system image
setup-emulator:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Installing Android emulator..."

    # Install emulator and system image (arm64 for Apple Silicon)
    sdkmanager "emulator" "system-images;android-34;google_apis;arm64-v8a"

    echo ""
    echo "Emulator installed. Run 'just emulator-create' to create a virtual device."

# Create an Android Virtual Device for testing
emulator-create name="cleanscope":
    #!/usr/bin/env bash
    set -euo pipefail
    AVD_NAME="{{name}}"

    # Check if AVD already exists
    if avdmanager list avd | grep -q "Name: $AVD_NAME"; then
        echo "AVD '$AVD_NAME' already exists. Delete it first with:"
        echo "  avdmanager delete avd -n $AVD_NAME"
        exit 1
    fi

    echo "Creating AVD '$AVD_NAME'..."
    echo "no" | avdmanager create avd \
        -n "$AVD_NAME" \
        -k "system-images;android-34;google_apis;arm64-v8a" \
        -d "pixel_6"

    echo ""
    echo "AVD '$AVD_NAME' created. Run 'just emulator-run' to start it."

# Install npm dependencies
install:
    npm install

# ============================================================================
# DEVELOPMENT
# ============================================================================

# Run desktop development server
dev:
    npm run tauri:dev

# Initialize or reinitialize Android project (run after changing package name/identifier)
android-init:
    rm -rf src-tauri/gen/android
    npx tauri android init

# Start Android emulator (run in separate terminal)
emulator-run name="cleanscope":
    #!/usr/bin/env bash
    set -euo pipefail
    SDK_ROOT="/opt/homebrew/share/android-commandlinetools"

    # Check if emulator exists
    if [[ ! -x "$SDK_ROOT/emulator/emulator" ]]; then
        echo "Emulator not installed. Run 'just setup-emulator' first."
        exit 1
    fi

    AVD_NAME="{{name}}"

    # Check if AVD exists
    if ! avdmanager list avd | grep -q "Name: $AVD_NAME"; then
        echo "AVD '$AVD_NAME' not found. Run 'just emulator-create' first."
        exit 1
    fi

    # Find AVD location (may be in non-standard location like ~/.config/.android/avd/)
    AVD_PATH=$(avdmanager list avd 2>/dev/null | grep "Path:" | head -1 | sed 's/.*Path: //')
    if [[ -n "$AVD_PATH" ]]; then
        export ANDROID_AVD_HOME="$(dirname "$AVD_PATH")"
    fi

    echo "Starting emulator '$AVD_NAME'..."
    echo "(Note: USB OTG is not supported in emulators - use physical device for endoscope testing)"
    echo ""
    "$SDK_ROOT/emulator/emulator" -avd "$AVD_NAME"

# Run Android development (builds, installs, launches on connected device)
android-dev: _require-android
    adb shell am force-stop com.cleanscope.app || true
    npm run tauri:android:dev

# Build Android APK (debug)
android-build: _require-android
    npm run tauri:android:build -- --debug

# Build Android APK (release)
android-release: _require-android
    npm run tauri:android:build

# Install APK to connected device
android-install: _require-android
    #!/usr/bin/env bash
    set -euo pipefail
    APK="src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
    if [[ ! -f "$APK" ]]; then
        # Try alternate path
        APK="src-tauri/gen/android/app/build/outputs/apk/debug/app-debug.apk"
    fi
    if [[ ! -f "$APK" ]]; then
        echo "APK not found. Run 'just android-build' first."
        exit 1
    fi
    echo "Installing $APK..."
    adb install -r "$APK"

# Uninstall app from device
android-uninstall:
    adb uninstall com.cleanscope.app || echo "App not installed"

# ============================================================================
# ADB & CONNECTIVITY
# ============================================================================

# Set up ADB over WiFi for USB endoscope testing
adb-wifi ip="":
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "{{ip}}" ]]; then
        echo "Enabling ADB TCP/IP mode..."
        adb tcpip 5555
        echo ""
        echo "Get your phone's IP address:"
        adb shell ip addr show wlan0 | grep "inet "
        echo ""
        echo "Then run: just adb-wifi <IP>"
    else
        echo "Connecting to {{ip}}:5555..."
        adb connect "{{ip}}:5555"
        echo ""
        echo "Verifying connection..."
        adb devices
        echo ""
        echo "You can now unplug the USB cable."
        echo "The endoscope can be plugged into the phone's USB-C port."
    fi

# Check ADB WiFi connection status
wifi-status:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== ADB Connection Status ==="
    echo ""

    # Get list of devices
    DEVICES=$(adb devices -l 2>/dev/null | tail -n +2 | grep -v "^$" || true)

    if [[ -z "$DEVICES" ]]; then
        echo "No devices connected."
        echo ""
        echo "To connect via WiFi, run: just wifi-setup"
        exit 0
    fi

    echo "Connected devices:"
    echo ""

    # Parse each device and show connection type
    while IFS= read -r line; do
        if [[ -z "$line" ]]; then continue; fi

        DEVICE_ID=$(echo "$line" | awk '{print $1}')
        STATUS=$(echo "$line" | awk '{print $2}')
        MODEL=$(echo "$line" | grep -oE "model:[^ ]+" | cut -d: -f2 || echo "unknown")

        if [[ "$DEVICE_ID" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+$ ]]; then
            CONNECTION_TYPE="WiFi"
            ICON="📶"
        else
            CONNECTION_TYPE="USB"
            ICON="🔌"
        fi

        echo "  $ICON $DEVICE_ID"
        echo "    Status: $STATUS"
        echo "    Model: $MODEL"
        echo "    Connection: $CONNECTION_TYPE"
        echo ""
    done <<< "$DEVICES"

    # Check if any WiFi connection exists
    if echo "$DEVICES" | grep -qE "^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+"; then
        echo "WiFi connection active - ready for USB endoscope testing!"
    else
        echo "Only USB connections found."
        echo "For endoscope testing, set up WiFi with: just wifi-setup"
    fi

# Connect to a known WiFi ADB device (no USB required if already paired)
wifi-connect ip="192.168.0.25" port="5555":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== ADB WiFi Connect ==="
    echo ""
    echo "Connecting to {{ip}}:{{port}}..."

    # Try to connect
    if adb connect "{{ip}}:{{port}}" 2>&1 | grep -qE "connected|already"; then
        echo ""
        echo "SUCCESS! Connected to {{ip}}:{{port}}"
        adb devices -l | grep -E "{{ip}}"
    else
        echo ""
        echo "Connection failed. The device may need to be re-paired."
        echo ""
        echo "Options:"
        echo "  1. Connect phone via USB and run: just wifi-setup"
        echo "  2. Use Android 11+ Wireless Debugging (no USB needed):"
        echo "     Settings > Developer Options > Wireless debugging"
        echo "     Then: adb pair <IP>:<PAIRING_PORT>"
        exit 1
    fi

# Complete WiFi setup workflow (requires USB for initial setup)
wifi-setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== ADB WiFi Setup ==="
    echo ""
    echo "This sets up wireless ADB so you can use the USB-C port for the endoscope."
    echo ""

    # Check if already connected via WiFi
    if adb devices | grep -qE "^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+.*device$"; then
        echo "Already connected via WiFi!"
        adb devices -l | grep -E "^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+"
        echo ""
        echo "You can unplug the USB cable and connect the endoscope."
        exit 0
    fi

    # Check if any device is connected (USB or otherwise)
    if ! adb devices | grep -qE "device$"; then
        echo "No device connected."
        echo ""
        echo "Options:"
        echo "  1. Connect phone via USB cable and run this again"
        echo "  2. If you know the IP, run: just wifi-connect <IP>"
        echo "  3. Use Android 11+ Wireless Debugging (no USB needed):"
        echo "     Settings > Developer Options > Wireless debugging"
        exit 1
    fi

    echo "Step 1: Enabling TCP/IP mode on port 5555..."
    adb tcpip 5555
    sleep 2

    echo ""
    echo "Step 2: Getting phone's WiFi IP address..."
    PHONE_IP=$(adb shell ip addr show wlan0 2>/dev/null | grep -oE "inet [0-9]+\.[0-9]+\.[0-9]+\.[0-9]+" | awk '{print $2}' || true)

    if [[ -z "$PHONE_IP" ]]; then
        echo ""
        echo "ERROR: Could not detect phone's IP address."
        echo "Make sure your phone is connected to WiFi."
        echo ""
        echo "You can find the IP manually in Settings > WiFi > (your network) > IP address"
        echo "Then run: just wifi-connect <IP>"
        exit 1
    fi

    echo "Phone IP: $PHONE_IP"
    echo ""
    echo "Step 3: Connecting via WiFi..."
    adb connect "$PHONE_IP:5555"
    sleep 1

    echo ""
    echo "Step 4: Verifying connection..."
    if adb devices | grep -qE "^$PHONE_IP:5555.*device$"; then
        echo ""
        echo "SUCCESS! Connected to $PHONE_IP:5555"
        echo ""
        echo "You can now:"
        echo "  1. Unplug the USB cable from your phone"
        echo "  2. Plug the USB endoscope into your phone's USB-C port"
        echo "  3. Run: just wifi-deploy    (to build and install the app)"
        echo "  4. Run: just logs           (to monitor the app)"
        echo ""
        echo "To reconnect later without USB: just wifi-connect $PHONE_IP"
    else
        echo ""
        echo "WARNING: Connection may have failed. Check with: just wifi-status"
    fi

# Default phone IP for quick reconnection (set to your phone's IP)
PHONE_IP := "192.168.0.25"

# Build and deploy to WiFi-connected device
wifi-deploy: _require-android
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== WiFi Deploy ==="
    echo ""

    # Check for any connection
    if ! adb devices | grep -qE "device$"; then
        echo "No device connected. Attempting to reconnect to {{PHONE_IP}}:5555..."
        if adb connect "{{PHONE_IP}}:5555" 2>&1 | grep -qE "connected|already"; then
            echo "Reconnected!"
        else
            echo ""
            echo "Could not connect. Options:"
            echo "  1. Run: just wifi-connect <IP>"
            echo "  2. Connect via USB and run: just wifi-setup"
            exit 1
        fi
    fi

    # Check for WiFi connection
    if ! adb devices | grep -qE "^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+.*device$"; then
        echo "WARNING: No WiFi connection detected."
        echo ""
        if adb devices | grep -qE "device$"; then
            echo "USB device found. Proceeding with USB deployment..."
        else
            echo "No device connected at all."
            echo "Run 'just wifi-connect' or 'just wifi-setup' first."
            exit 1
        fi
    else
        echo "WiFi connection active."
    fi

    echo ""
    echo "Building Android APK (debug)..."
    npm run tauri:android:build -- --debug

    echo ""
    echo "Stopping existing app..."
    adb shell am force-stop com.cleanscope.app || true

    echo ""
    echo "Installing APK to device..."
    APK="src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
    if [[ ! -f "$APK" ]]; then
        APK="src-tauri/gen/android/app/build/outputs/apk/debug/app-debug.apk"
    fi
    if [[ ! -f "$APK" ]]; then
        echo "ERROR: APK not found after build."
        exit 1
    fi
    adb install -r "$APK"

    echo ""
    echo "Launching app..."
    adb shell am start -n com.cleanscope.app/.MainActivity

    echo ""
    echo "Deploy complete! The app should now be running on your device."
    echo ""
    echo "To view logs: just logs"

# Full USB endoscope testing workflow
endoscope-test: _require-android
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== USB Endoscope Testing Workflow ==="
    echo ""

    # Step 1: Check WiFi connection
    echo "Checking ADB connection..."
    if adb devices | grep -qE "^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+.*device$"; then
        WIFI_IP=$(adb devices | grep -oE "^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+" | head -1)
        echo "WiFi connection active: $WIFI_IP"
    elif adb devices | grep -qE "device$"; then
        echo "Only USB connection found."
        echo ""
        echo "For endoscope testing, you need WiFi ADB (the USB port is needed for the endoscope)."
        echo ""
        read -p "Set up WiFi connection now? [Y/n] " -n 1 -r
        echo ""
        if [[ ! $REPLY =~ ^[Nn]$ ]]; then
            just wifi-setup
            echo ""
            echo "Now run 'just endoscope-test' again after unplugging USB."
            exit 0
        else
            echo "Continuing with USB (endoscope cannot be connected)..."
        fi
    else
        echo "ERROR: No device connected."
        echo ""
        echo "Connect your phone via USB and run: just wifi-setup"
        exit 1
    fi

    echo ""
    echo "Building and deploying app..."
    npm run tauri:android:build -- --debug

    APK="src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
    if [[ ! -f "$APK" ]]; then
        APK="src-tauri/gen/android/app/build/outputs/apk/debug/app-debug.apk"
    fi
    adb install -r "$APK"

    echo ""
    echo "Launching app..."
    adb shell am start -n com.cleanscope.app/.MainActivity

    echo ""
    echo "============================================"
    echo "  READY FOR ENDOSCOPE TESTING"
    echo "============================================"
    echo ""
    echo "1. Make sure USB cable is UNPLUGGED from phone"
    echo "2. Plug the USB endoscope into phone's USB-C port"
    echo "3. The app should auto-launch and request permission"
    echo "4. Watch the logs below for frame data..."
    echo ""
    echo "Press Ctrl+C to stop log streaming"
    echo "============================================"
    echo ""

    # Stream logs
    adb logcat -s CleanScope:* RustStdoutStderr:* AndroidRuntime:E

# List connected Android devices
devices:
    adb devices -l

# ============================================================================
# DEVICE & LOGS
# ============================================================================

# Show USB-related logs and device info
usb-debug:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Connected USB Devices ==="
    adb shell dumpsys usb | grep -A5 "USB Device" || echo "No USB devices"
    echo ""
    echo "=== Recent USB Logs ==="
    adb logcat -d | grep -E "(USB|UVC|libusb|CleanScope)" | tail -50

# Force stop the app
force-stop:
    adb shell am force-stop com.cleanscope.app
    @echo "App stopped"

# Launch the app without rebuilding
launch:
    adb shell am start -n com.cleanscope.app/.MainActivity
    @echo "App launched"

# Uninstall and reinstall (clears app data)
reinstall: android-uninstall android-install launch
    @echo "App reinstalled with fresh data"

# Stream app logs from device
logs:
    adb logcat -s CleanScope:* RustStdoutStderr:* AndroidRuntime:E

# Stream all logs (verbose)
logs-all:
    adb logcat

# Clear device logs
logs-clear:
    adb logcat -c

# Open shell on device
shell:
    adb shell

# ============================================================================
# CODE QUALITY
# ============================================================================

# Run all checks (lint, format, typecheck, rust)
check-all: lint format-check typecheck rust-check

# Lint frontend code
lint:
    npm run lint

# Fix lint issues
lint-fix:
    npm run lint:fix

# Check formatting
format-check:
    npm run format:check

# Format code
format:
    npm run format

# TypeScript/Svelte type checking
typecheck:
    npm run check

# Rust checks
rust-check:
    cd src-tauri && cargo check

# Rust lints
rust-clippy:
    cd src-tauri && cargo clippy

# Run all Rust checks
rust-all: rust-check rust-clippy

# ============================================================================
# BUILD
# ============================================================================

# Build frontend only
build-frontend:
    npm run build

# Build desktop app
build-desktop:
    npm run tauri:build

# Clean build artifacts
clean:
    rm -rf dist
    rm -rf src-tauri/target
    rm -rf src-tauri/gen/android/app/build

# ============================================================================
# ADR & DOCUMENTATION
# ============================================================================

# List all Architecture Decision Records
adr-list:
    @echo "Architecture Decision Records:"
    @ls -1 .claude/blueprints/adr/*.md 2>/dev/null | grep -v manifest | while read f; do \
        title=$$(head -1 "$$f" | sed 's/^# //'); \
        echo "  $$(basename "$$f"): $$title"; \
    done

# Show frame streaming statistics from logs
frame-stats:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Frame Streaming Statistics ==="
    LOGS=$$(adb logcat -d | grep -E "MJPEG frame|Received.*frames")
    echo "$$LOGS" | tail -20
    echo ""
    FRAME_COUNT=$$(echo "$$LOGS" | grep -oE "Received [0-9]+ frames" | tail -1)
    echo "Latest: $$FRAME_COUNT"

# ============================================================================
# INTERNAL RECIPES
# ============================================================================

# Verify Android environment is set up
_require-android:
    #!/usr/bin/env bash
    if [[ -z "${ANDROID_HOME:-}" ]] || [[ -z "${NDK_HOME:-}" ]]; then
        echo "Error: Android environment not configured."
        echo "Run 'just check-prereqs' to see what's missing."
        echo "Run 'just setup' to install prerequisites."
        exit 1
    fi
