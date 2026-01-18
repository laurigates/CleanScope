# Work Overview: CleanScope

## Current Phase: Reliability & Error Handling

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
- Resolution cycling UI (ADR-003)
- USB disconnection error handling (F016)
  - StopReason tracking in isochronous stream
  - Structured usb-error events with error types
  - Frontend error state with context-aware UI

### Architecture Decision Records
- ADR-001: Frame Streaming Architecture (polling pattern)
- ADR-002: JPEG Decoding Strategy (browser-native)
- ADR-003: Resolution Negotiation Protocol (progressive)

### Pending
- Test on physical Android device with USB endoscope
- Frame rate optimization
- Reconnection handling (auto-reconnect on device reattachment)

## Next Steps
1. Deploy to physical device for end-to-end testing
2. Test USB disconnection scenarios
3. Implement auto-reconnection logic
