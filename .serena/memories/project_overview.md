# CleanScope - Project Overview

## Purpose
Privacy-respecting Android application for viewing video feeds from USB-C endoscopes (UVC devices). Built with Tauri v2 + Rust to avoid Java/Kotlin complexity.

## Tech Stack
- **Tauri v2**: App shell, APK bundling, WebView
- **Rust**: Core logic, USB handling, performance-critical code
- **Svelte 5**: Frontend UI with runes syntax (`$state`, `$derived`)
- **TypeScript**: Frontend type safety
- **JNI**: Bridge to Android APIs for USB file descriptors
- **UVC**: USB Video Class protocol for camera communication

## Project Structure
```
CleanScope/
├── src/                    # Svelte frontend
│   ├── App.svelte          # Main component
│   └── main.ts             # Entry point
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── lib.rs          # Tauri commands, app setup
│   │   └── usb.rs          # USB/UVC handling (Android-only via JNI)
│   ├── gen/android/        # Android configuration
│   ├── Cargo.toml          # Rust dependencies
│   └── tauri.conf.json     # Tauri config
└── package.json            # Frontend dependencies
```

## Key Architecture Patterns
- **Platform-specific code**: Use `#[cfg(target_os = "android")]` for Android-only paths
- **Frontend-backend communication**: Tauri commands (`invoke()`) and events (`listen()`, `emit()`)
- **USB flow**: Android intent → permission → JNI → file descriptor → UVC streaming
