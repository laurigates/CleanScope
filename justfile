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
# DEVICE & LOGS
# ============================================================================

# List connected Android devices
devices:
    adb devices -l

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
