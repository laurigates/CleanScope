# Work Overview: CleanScope

## Current Phase: Scaffolding

### Completed
- Tauri v2 + Rust Android app scaffolded
- Svelte 5 frontend with runes syntax
- Android USB intent filters configured
- JNI bridge for USB file descriptor access
- Basic UI with connection status display

### In Progress
- UVC camera streaming implementation

### Pending
- Integrate UVC library with vendored libusb for Android
- Implement MJPEG frame decoding pipeline
- Canvas rendering for camera frames
- Resolution cycling functionality
- Error handling for USB disconnection

## Next Steps
1. Find/fork uvc-rs with Android-compatible vendored libusb
2. Implement frame callback and streaming loop
3. Connect frame data to frontend canvas
