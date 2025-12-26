# Work Overview: CleanScope

## Current Phase: UVC Streaming

### Completed
- Tauri v2 + Rust Android app scaffolded
- Svelte 5 frontend with runes syntax
- Android USB intent filters configured
- JNI bridge for USB file descriptor access
- Basic UI with connection status display
- libusb1-sys integration with vendored libusb for Android
- libusb Android wrapper (libusb_android.rs) with safe FFI
- UVC streaming protocol implementation (probe/commit, bulk transfers)
- MJPEG frame boundary detection

### In Progress
- Connect MJPEG frames to frontend canvas

### Pending
- JPEG decoding with jpeg-decoder crate
- Canvas rendering for camera frames
- Resolution cycling functionality
- Error handling for USB disconnection
- Frame rate optimization

## Next Steps
1. Emit decoded frames to frontend via Tauri events
2. Render frames on HTML5 canvas
3. Test with physical USB endoscope device
