# CleanScope Refactoring Plan

> Generated from comprehensive codebase analysis on 2026-01-10

## Executive Summary

This plan addresses code smells, architectural issues, and test coverage gaps identified in the CleanScope codebase. Items are prioritized by impact and risk.

---

## Priority 1: Critical Bug Fixes

### 1.1 Fix undefined `context` variable in `extract_urb_payloads`

**Location:** `src-tauri/src/libusb_android.rs:1643-1647`

**Problem:** The function references `context.capture_state` but `context` is not a parameter:

```rust
// BUG: context is not defined in this function's scope
if let Some(capture_state) = &context.capture_state {
    if capture_state.is_capturing() {
        capture_state.add_packet(pkt_data, xfr.endpoint);
    }
}
```

**Solution:** Either:
- Pass `capture_state: Option<&CaptureState>` as a parameter, or
- Move this logic back to the caller (`iso_transfer_callback_inner`)

**Risk:** High - This is either a compilation error or dead code path

**Effort:** Small (1-2 hours)

---

## Priority 2: Code Duplication

### 2.1 Consolidate duplicate `validate_uvc_header` implementations

**Locations:**
- `src-tauri/src/libusb_android.rs:1548-1589`
- `src-tauri/src/frame_assembler.rs:344-363`

**Problem:** Same validation logic duplicated, divergent maintenance risk.

**Solution:**
1. Keep the tested implementation in `frame_assembler.rs`
2. Export it as `pub fn validate_uvc_header(...)`
3. Import and use in `libusb_android.rs`

**Effort:** Small (1-2 hours)

---

### 2.2 Remove or document dead code `round_to_yuy2_frame_size`

**Location:** `src-tauri/src/libusb_android.rs:1495-1535`

**Problem:** Function is defined but never called.

**Solution:** Either:
- Remove entirely if not needed, or
- Add `#[allow(dead_code)]` with documentation explaining future use, or
- Actually use it where frame size rounding is needed

**Effort:** Small (30 minutes)

---

### 2.3 Extract common event loop spawning pattern

**Locations:**
- `src-tauri/src/usb.rs:1149-1175` (format detection)
- `src-tauri/src/usb.rs:1368-1401` (legacy streaming)
- `src-tauri/src/usb.rs:1529-1555` (YUY2 streaming)

**Problem:** Event loop spawning code duplicated 3 times with minor variations.

**Solution:** Extract a common function:

```rust
fn spawn_libusb_event_loop(
    ctx_ptr: SendableContextPtr,
    stop_flag: Arc<AtomicBool>,
    name: &'static str,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                unsafe {
                    libusb1_sys::libusb_handle_events_timeout_completed(
                        ctx_ptr.0,
                        &LIBUSB_TIMEVAL,
                        std::ptr::null_mut(),
                    );
                }
            }
        })
        .expect("Failed to spawn event loop thread")
}
```

**Effort:** Medium (2-3 hours)

---

### 2.4 Extract common frame emission pattern

**Locations:**
- `src-tauri/src/libusb_android.rs:1750-1755` (MJPEG FID toggle)
- `src-tauri/src/libusb_android.rs:1823-1833` (MJPEG EOF)
- `src-tauri/src/libusb_android.rs:1791-1818` (YUY2 size-based)

**Problem:** Similar patterns for sending completed frames.

**Solution:** Extract:

```rust
fn emit_completed_frame(
    state: &mut SharedFrameState,
    context: &IsoCallbackContext,
    trigger: FrameTrigger,
) {
    let frame_data = state.frame_buffer.drain(..).collect::<Vec<_>>();
    if !frame_data.is_empty() {
        let _ = context.frame_sender.send(frame_data);
        state.frames_sent += 1;
        log_frame_emission(trigger, state.frames_sent);
    }
}
```

**Effort:** Medium (2-3 hours)

---

### 2.5 Consolidate cycling functions into generic helper

**Locations:**
- `src-tauri/src/lib.rs:437-455` (`cycle_width`)
- `src-tauri/src/lib.rs:458-476` (`cycle_height`)
- `src-tauri/src/lib.rs:479-495` (`cycle_stride`)

**Problem:** Three nearly identical functions with only option array differing.

**Solution:**

```rust
fn cycle_option<T: Copy + ToString>(
    index: &mut Option<usize>,
    options: &[T],
    prefix: &str,
) -> String {
    let new_index = match *index {
        None => Some(0),
        Some(i) if i + 1 < options.len() => Some(i + 1),
        Some(_) => None,
    };
    *index = new_index;
    match new_index {
        None => format!("{}:Auto", prefix),
        Some(i) => format!("{}:{}", prefix, options[i]),
    }
}

#[tauri::command]
fn cycle_width(state: State<'_, AppState>) -> String {
    let mut index = state.width_index.lock().map_err(|e| e.to_string())?;
    cycle_option(&mut index, &WIDTH_OPTIONS, "W")
}
```

**Effort:** Small (1-2 hours)

---

### 2.6 Extract JPEG detection utility

**Locations:**
- `src-tauri/src/lib.rs:317-318`
- `src-tauri/src/lib.rs:338-343`
- `src-tauri/src/lib.rs:422-426`

**Problem:** JPEG magic byte detection duplicated.

**Solution:**

```rust
/// Check if data starts with JPEG magic bytes (0xFFD8)
fn is_jpeg_data(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8
}
```

**Effort:** Small (30 minutes)

---

## Priority 3: Long Functions

### 3.1 Refactor `stream_frames_yuy2` (355 lines)

**Location:** `src-tauri/src/usb.rs:1456-1811`

**Problem:** Single function handles too many responsibilities:
- Stream initialization
- Event loop management
- Frame reception
- Dimension calculation
- Format conversion dispatch
- Frame buffer updates
- Settings change detection
- Restart handling

**Solution:** Extract focused functions:

```rust
// New structure
fn stream_frames_yuy2(...) -> Result<(), String> {
    let mut stream_context = StreamContext::new(...)?;

    loop {
        if should_stop(&stream_context) {
            break;
        }

        match receive_frame(&stream_context) {
            Ok(frame) => {
                let dimensions = calculate_frame_dimensions(&frame, &stream_context)?;
                let rgb_data = convert_frame_to_rgb(&frame, &dimensions)?;
                update_frame_buffer(&stream_context, rgb_data, &dimensions)?;
                emit_frame_event(&stream_context.app_handle)?;
            }
            Err(e) => handle_frame_error(e),
        }

        if detect_settings_change(&stream_context) {
            handle_restart(&stream_context)?;
        }
    }

    Ok(())
}

// Extracted functions:
fn calculate_frame_dimensions(frame: &[u8], ctx: &StreamContext) -> Result<FrameDimensions, String>;
fn convert_frame_to_rgb(frame: &[u8], dims: &FrameDimensions) -> Result<Vec<u8>, String>;
fn update_frame_buffer(ctx: &StreamContext, rgb: Vec<u8>, dims: &FrameDimensions) -> Result<(), String>;
fn detect_settings_change(ctx: &StreamContext) -> bool;
```

**Effort:** Large (4-6 hours)

---

### 3.2 Refactor `run_camera_loop_inner` (280 lines)

**Location:** `src-tauri/src/usb.rs:405-682`

**Problem:** Handles device setup, format discovery, format selection, and streaming dispatch with deeply nested conditionals.

**Solution:** Extract:

```rust
fn run_camera_loop_inner(...) -> Result<(), String> {
    let device = setup_usb_device(...)?;
    let formats = discover_formats(&device)?;
    store_available_formats(&formats, &app_handle)?;

    let selected_format = select_streaming_format(&formats, &streaming_config)?;

    match selected_format {
        Format::Mjpeg(config) => stream_mjpeg(config, ...),
        Format::Yuy2(config) => stream_yuy2(config, ...),
    }
}

// Extracted functions:
fn discover_formats(device: &UsbDevice) -> Result<Vec<FormatDescriptor>, String>;
fn store_available_formats(formats: &[FormatDescriptor], app: &AppHandle) -> Result<(), ()>;
fn select_streaming_format(formats: &[FormatDescriptor], config: &StreamingConfig) -> Result<Format, String>;
fn try_mjpeg_detection(device: &UsbDevice, timeout: Duration) -> Option<MjpegConfig>;
```

**Effort:** Large (4-6 hours)

---

## Priority 4: Architecture Improvements

### 4.1 Introduce `StreamingContext` struct

**Location:** `src-tauri/src/usb.rs:39-48`

**Problem:** `init_usb_handler` takes 8 parameters, 6 of which are `Arc<Mutex<T>>`.

**Solution:**

```rust
pub struct StreamingContext {
    pub app_handle: AppHandle,
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
    pub display_settings: Arc<Mutex<DisplaySettings>>,
    pub streaming_config: Arc<Mutex<StreamingConfig>>,
    pub width_index: Arc<Mutex<Option<usize>>>,
    pub stride_index: Arc<Mutex<Option<usize>>>,
    pub stop_flag: Arc<AtomicBool>,
    pub validation_level: ValidationLevel,
}

impl StreamingContext {
    pub fn from_app_state(app_handle: AppHandle, state: &AppState) -> Self { ... }
}

// Simplified function signature:
pub fn init_usb_handler(ctx: StreamingContext) { ... }
```

**Effort:** Medium (3-4 hours)

---

### 4.2 Consolidate `AppState` fields into logical groups

**Location:** `src-tauri/src/lib.rs:129-150`

**Problem:** 10 separate `Arc<Mutex<>>` fields cause lock contention and complexity.

**Solution:**

```rust
pub struct DisplayState {
    pub settings: DisplaySettings,
    pub width_index: Option<usize>,
    pub height_index: Option<usize>,
    pub stride_index: Option<usize>,
    pub offset_index: usize,
}

pub struct AppState {
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
    pub display_state: Arc<Mutex<DisplayState>>,  // Consolidated
    pub streaming_config: Arc<Mutex<StreamingConfig>>,
    pub capture_state: Arc<CaptureState>,
    pub usb_stop_flag: Arc<AtomicBool>,
    pub validation_level: ValidationLevel,
}
```

**Effort:** Medium (3-4 hours)

---

### 4.3 Group magic numbers into configuration structs

**Locations:**
- `src-tauri/src/usb.rs:325-333`
- `src-tauri/src/libusb_android.rs:1026-1042`
- `src-tauri/src/frame_validation.rs:12-14`

**Solution:**

```rust
pub struct UvcConfig {
    pub max_format_index: u8,
    pub frames_to_check_format: u32,
    pub format_detection_timeout_secs: u64,
}

pub struct IsoTransferConfig {
    pub packets_per_transfer: i32,
    pub num_transfers: usize,
    pub event_timeout_ms: i32,
}

pub struct ValidationConfig {
    pub row_diff_threshold: f64,
    pub size_tolerance_strict: f64,
    pub size_tolerance_moderate: f64,
}

impl Default for UvcConfig { ... }
impl Default for IsoTransferConfig { ... }
impl Default for ValidationConfig { ... }
```

**Effort:** Medium (2-3 hours)

---

## Priority 5: Error Handling

### 5.1 Replace mutex `.unwrap()` with proper error handling

**Locations:** Multiple in `src-tauri/src/lib.rs`

**Problem:** `.unwrap()` on mutex locks panics the app if lock is poisoned.

**Solution:**

```rust
// Before:
let mut index = state.width_index.lock().unwrap();

// After:
let mut index = state.width_index.lock()
    .map_err(|e| format!("Lock poisoned: {}", e))?;
```

**Effort:** Small (1-2 hours)

---

### 5.2 Create unified `AppError` type for Tauri commands

**Location:** `src-tauri/src/lib.rs`

**Problem:** Commands use `Result<T, String>` losing error context.

**Solution:**

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Lock error: {0}")]
    LockPoisoned(String),

    #[error("USB error: {0}")]
    Usb(#[from] LibusbError),

    #[error("Frame error: {0}")]
    Frame(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        serializer.serialize_str(&self.to_string())
    }
}
```

**Effort:** Medium (2-3 hours)

---

## Priority 6: Performance

### 6.1 Only clone frame data when capturing

**Location:** `src-tauri/src/usb.rs:1767`

**Problem:** Every frame is cloned for `raw_frame` even when not capturing, wasting ~54MB/s at 30fps 720p.

**Solution:**

```rust
// Before:
buffer.raw_frame = frame_data.clone();

// After:
if capture_state.is_capturing() {
    buffer.raw_frame = frame_data.clone();
}
```

**Effort:** Small (30 minutes)

---

### 6.2 Combine `get_frame` and `get_frame_info` IPC calls

**Location:** `src/App.svelte:145-163`

**Problem:** Two round-trip IPC calls per frame adds latency.

**Solution:**

```rust
#[derive(Serialize)]
pub struct FrameResponse {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub data: Vec<u8>,
}

#[tauri::command]
fn get_frame_with_info(state: State<'_, AppState>) -> Result<FrameResponse, String> {
    // Return both info and data in single call
}
```

**Effort:** Small (1-2 hours)

---

## Priority 7: Test Coverage

### 7.1 Extract YUV conversion to platform-independent module

**Location:** `src-tauri/src/usb.rs:814-1098`

**Problem:** YUV conversion functions are behind `#[cfg(target_os = "android")]`, untestable in CI.

**Solution:**

```rust
// New file: src-tauri/src/yuv_conversion.rs
pub fn convert_yuv422_to_rgb(
    yuv_data: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    format: YuvFormat,
) -> Result<Vec<u8>, ConversionError> { ... }

pub fn convert_i420_to_rgb(...) -> Result<Vec<u8>, ConversionError> { ... }
pub fn convert_nv12_to_rgb(...) -> Result<Vec<u8>, ConversionError> { ... }

#[cfg(test)]
mod tests {
    #[test]
    fn yuv422_to_rgb_basic() { ... }

    #[test]
    fn yuv422_handles_stride_padding() { ... }

    #[test]
    fn roundtrip_preserves_luminance() { ... }
}
```

**Effort:** Large (4-6 hours)

---

### 7.2 Add Tauri command unit tests

**Location:** `src-tauri/src/lib.rs`

**Solution:**

```rust
#[cfg(test)]
mod command_tests {
    use super::*;

    fn create_test_state() -> AppState {
        AppState::default()
    }

    #[test]
    fn cycle_width_wraps_to_auto() {
        let state = create_test_state();
        for _ in 0..WIDTH_OPTIONS.len() {
            let _ = cycle_width(State(&state));
        }
        let result = cycle_width(State(&state));
        assert!(result.contains("Auto"));
    }

    #[test]
    fn toggle_skip_mjpeg_toggles() {
        let state = create_test_state();
        let first = toggle_skip_mjpeg(State(&state));
        let second = toggle_skip_mjpeg(State(&state));
        assert_ne!(first, second);
    }
}
```

**Effort:** Medium (3-4 hours)

---

### 7.3 Add frame processing integration tests

**Solution:**

```rust
// New file: src-tauri/tests/frame_pipeline_test.rs
use cleanscope::frame_assembler::FrameAssembler;
use cleanscope::yuv_conversion::convert_yuv422_to_rgb;
use cleanscope::test_utils::PacketGenerator;

#[test]
fn complete_pipeline_packet_to_rgb() {
    let generator = PacketGenerator::new();
    let packets = generator.generate_yuy2_frame(640, 480, Color::RED);

    let mut assembler = FrameAssembler::new(640 * 480 * 2);
    for packet in packets {
        assembler.add_packet(&packet);
    }

    let frame = assembler.get_completed_frame().unwrap();
    let rgb = convert_yuv422_to_rgb(&frame, 640, 480, 640 * 2, YuvFormat::Yuyv).unwrap();

    // Verify red-ish color in RGB
    assert!(rgb[0] > 200); // R
    assert!(rgb[1] < 50);  // G
    assert!(rgb[2] < 50);  // B
}
```

**Effort:** Medium (3-4 hours)

---

## Summary

| Priority | Items | Total Effort |
|----------|-------|--------------|
| P1: Critical Bugs | 1 | 1-2 hours |
| P2: Duplication | 6 | 8-12 hours |
| P3: Long Functions | 2 | 8-12 hours |
| P4: Architecture | 3 | 8-11 hours |
| P5: Error Handling | 2 | 3-5 hours |
| P6: Performance | 2 | 1.5-2.5 hours |
| P7: Test Coverage | 3 | 10-14 hours |
| **Total** | **19 items** | **39.5-58.5 hours** |

---

## Implementation Order

Recommended sequence to minimize risk and maximize incremental value:

1. **P1.1** - Fix critical bug (blocks nothing, high risk if unfixed)
2. **P2.1** - Consolidate `validate_uvc_header` (enables P7.1)
3. **P2.2** - Remove dead code (quick win)
4. **P5.1** - Fix mutex unwraps (prevents crashes)
5. **P6.1** - Conditional frame cloning (quick performance win)
6. **P2.6** - Extract JPEG detection (quick, reduces duplication)
7. **P2.5** - Consolidate cycling functions (quick, reduces duplication)
8. **P4.1** - Introduce `StreamingContext` (enables P3.1, P3.2)
9. **P3.1** - Refactor `stream_frames_yuy2`
10. **P3.2** - Refactor `run_camera_loop_inner`
11. **P7.1** - Extract YUV conversion module (enables testing)
12. **P7.2** - Add Tauri command tests
13. **P7.3** - Add integration tests
14. **P2.3** - Extract event loop pattern
15. **P2.4** - Extract frame emission pattern
16. **P4.2** - Consolidate AppState
17. **P4.3** - Group magic numbers
18. **P5.2** - Create AppError type
19. **P6.2** - Combine IPC calls
