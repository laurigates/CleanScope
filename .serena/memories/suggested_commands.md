# CleanScope - Development Commands

## Frontend (from project root)

```bash
# Install dependencies
npm install

# Development server (desktop)
npm run dev

# Build frontend
npm run build

# Type checking
npm run check

# Linting
npm run lint
npm run lint:fix

# Formatting
npm run format
npm run format:check
```

## Tauri / Full App

```bash
# Desktop development
npm run tauri:dev

# Android development (requires SDK + NDK)
npm run tauri:android:dev

# Build Android APK
npm run tauri:android:build
```

## Rust Backend (from src-tauri/)

```bash
# Check compilation
cargo check

# Run Clippy lints
cargo clippy

# Format code
cargo fmt

# Run tests
cargo test
```

## Android Debugging

```bash
# View app logs (filter by tag)
adb logcat -s CleanScope:* RustStdoutStderr:*

# List connected devices
adb devices
```

## Pre-commit Hooks

```bash
# Install hooks
pre-commit install --install-hooks

# Run all hooks manually
pre-commit run --all-files

# Run specific hook
pre-commit run biome-check --all-files
pre-commit run cargo-clippy --all-files
```

## System Utilities (macOS/Darwin)

```bash
# File search
fd <pattern>              # Fast file finding
rg <pattern>              # Fast text search

# Git
git status
git diff
git log --oneline -10

# Directory listing
ls -la
```
