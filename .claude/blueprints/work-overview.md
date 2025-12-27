# Work Overview: CleanScope

## Current Phase: Testing & Refinement

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
- Frame streaming architecture (ADR-001)
  - FrameBuffer shared state with Arc<Mutex<>>
  - get_frame command with ipc::Response
  - Lightweight "frame-ready" event emission
- Frontend canvas rendering with createImageBitmap()

### Architecture Decision Records
- ADR-001: Frame Streaming Architecture (polling pattern)
- ADR-002: JPEG Decoding Strategy (browser-native)
- ADR-003: Resolution Negotiation Protocol (progressive)

### Pending
- Test on physical Android device with USB endoscope
- Resolution cycling UI implementation
- Error handling for USB disconnection
- Frame rate optimization
- Reconnection handling

## Next Steps
1. Deploy to physical device for end-to-end testing
2. Implement resolution cycling based on ADR-003
3. Add error recovery and reconnection logic
