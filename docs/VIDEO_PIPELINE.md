# CleanScope Video Pipeline - Complete Technical Reference

This document provides comprehensive documentation of all parameters, processing stages, and data flow in the CleanScope video pipeline from USB camera to display.

## Pipeline Overview

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│   Camera     │───▶│    UVC       │───▶│ Isochronous  │───▶│    Frame     │───▶│    Frame     │───▶│   YUV→RGB    │───▶│   Frontend   │
│  Discovery   │    │ Negotiation  │    │  Transfers   │    │   Assembly   │    │  Validation  │    │  Conversion  │    │   Display    │
└──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
     Stage 1             Stage 2             Stage 3             Stage 4             Stage 5             Stage 6             Stage 7
```

---

## Stage 1: Device Discovery & Format Detection

**File:** `usb.rs` - `init_usb_handler()`

### What Happens
1. Android detects USB device → triggers `USB_DEVICE_ATTACHED` intent
2. App auto-launches via `AndroidManifest.xml` intent filter
3. Permission auto-granted via `device_filter.xml` matching
4. JNI provides file descriptor from `UsbDeviceConnection`
5. `get_format_descriptors()` reads USB descriptors
6. `parse_format_descriptors()` extracts available formats and resolutions

### Parameters Discovered

#### Format Types (from VS_FORMAT_* descriptors)
| Format | Descriptor Subtype | GUID | Bytes/Pixel |
|--------|-------------------|------|-------------|
| MJPEG | 0x06 (VS_FORMAT_MJPEG) | N/A | Variable |
| YUYV/YUY2 | 0x04 (VS_FORMAT_UNCOMPRESSED) | `59 55 59 32...` | 2.0 |
| UYVY | 0x04 (VS_FORMAT_UNCOMPRESSED) | `55 59 56 59...` | 2.0 |
| NV12 | 0x04 (VS_FORMAT_UNCOMPRESSED) | `4E 56 31 32...` | 1.5 |
| I420 | 0x04 (VS_FORMAT_UNCOMPRESSED) | `49 34 32 30...` | 1.5 |
| RGB24 | 0x04 (VS_FORMAT_UNCOMPRESSED) | `7D EB 36 E4...` | 3.0 |
| BGR24 | 0x04 (VS_FORMAT_UNCOMPRESSED) | `E4 36 EB 7D...` | 3.0 |

#### Per-Format Frame Descriptors
| Field | Type | Description |
|-------|------|-------------|
| `format_index` | u8 | 1-based format identifier |
| `frame_index` | u8 | 1-based resolution identifier within format |
| `width` | u16 | Resolution width in pixels |
| `height` | u16 | Resolution height in pixels |
| `max_frame_size` | u32 | Declared max frame size (often incorrect on cheap cameras) |

### Output
- `Vec<DiscoveredFormat>` populates `StreamingConfig.available_formats`
- UI can display available formats via `get_available_formats()` command

---

## Stage 2: UVC Probe/Commit Negotiation

**File:** `usb.rs` - `start_uvc_streaming_with_resolution()`

### What Happens
1. App sends `SET_CUR PROBE` with desired format_index and frame_index
2. Camera responds with `GET_CUR PROBE` containing actual negotiated parameters
3. App sends `SET_CUR COMMIT` to confirm and activate streaming
4. `set_interface_alt_setting()` enables high-bandwidth isochronous endpoint

### UVC Stream Control Structure (26 bytes)
| Offset | Field | Type | Description |
|--------|-------|------|-------------|
| 0-1 | `bm_hint` | u16 | Hint bits (bit 0 = frame interval valid) |
| 2 | `b_format_index` | u8 | Selected format (1-based) |
| 3 | `b_frame_index` | u8 | Selected resolution (1-based) |
| 4-7 | `dw_frame_interval` | u32 | Frame interval in 100ns units (e.g., 333333 = 30fps) |
| 8-9 | `w_key_frame_rate` | u16 | Key frame rate (MJPEG) |
| 10-11 | `w_p_frame_rate` | u16 | P-frame rate (MJPEG) |
| 12-13 | `w_comp_quality` | u16 | JPEG quality (0-100) |
| 14-15 | `w_comp_window_size` | u16 | Compression window |
| 16-17 | `w_delay` | u16 | Latency |
| 18-21 | `dw_max_video_frame_size` | u32 | Max frame size **← OFTEN INCORRECT** |
| 22-25 | `dw_max_payload_transfer_size` | u32 | Max USB payload per transfer |

### Critical Note
The `dw_max_video_frame_size` from probe/commit response is **frequently wrong** on cheap endoscopes. CleanScope uses the descriptor-provided frame size instead.

### Output
```rust
struct UvcNegotiatedParams {
    endpoint: u8,           // USB endpoint address (e.g., 0x81)
    format_index: u8,       // Confirmed format
    frame_index: u8,        // Confirmed resolution
    width: u16,             // From descriptor lookup, NOT probe
    height: u16,            // From descriptor lookup, NOT probe
    max_frame_size: u32,    // For logging/validation
}
```

---

## Stage 3: Isochronous USB Transfers

**File:** `libusb_android.rs` - `IsochronousStream::new()`, `setup_and_submit_transfer()`

### What Happens
1. Create 4 transfer objects, each with 32 isochronous packets
2. Allocate buffers: `NUM_TRANSFERS × ISO_PACKETS_PER_TRANSFER × max_packet_size`
3. Submit all 4 transfers to libusb queue (pipeline for continuous streaming)
4. Spawn event loop thread to process callbacks

### Constants
| Constant | Value | Purpose |
|----------|-------|---------|
| `ISO_PACKETS_PER_TRANSFER` | 32 | Packets per USB transfer |
| `NUM_TRANSFERS` | 4 | Concurrent transfers in flight |
| `EVENT_TIMEOUT_MS` | 100 | libusb event loop timeout (ms) |

### Transfer Pool Architecture
```
Transfer 0: [Packet 0-31] ──────┐
Transfer 1: [Packet 0-31] ──────┼──▶ Event Loop ──▶ iso_transfer_callback()
Transfer 2: [Packet 0-31] ──────┤
Transfer 3: [Packet 0-31] ──────┘

Total: 128 packets in flight at any time
Buffer: ~512 KB (4 × 32 × 1024 typical)
```

### Output
- `mpsc::Receiver<Vec<u8>>` - Frame receiver channel
- Event loop thread polling for transfer completions

---

## Stage 4: Frame Assembly

**File:** `libusb_android.rs` - `process_iso_packets()`, `validate_uvc_header()`

### What Happens
1. Each isochronous packet arrives with 2-12 byte UVC header
2. Header is parsed and stripped
3. Payload bytes are accumulated into frame buffer
4. Frame boundary detection triggers frame emission

### UVC Header Structure
| Byte | Field | Description |
|------|-------|-------------|
| 0 | Length | Header length (2-12 bytes) |
| 1 | Flags | Bit flags (see below) |
| 2-5 | PTS | Presentation timestamp (optional, if bit 2 set) |
| 6-11 | SCR | Source clock reference (optional, if bit 3 set) |

### Header Flag Bits (Byte 1)
| Bit | Mask | Name | Description |
|-----|------|------|-------------|
| 7 | 0x80 | EOH | End of Header (MUST be set) |
| 6 | 0x40 | ERR | Error (frame corruption) |
| 5 | 0x20 | STI | Still image |
| 4 | 0x10 | RES | Reserved |
| 3 | 0x08 | SCR | SCR field present |
| 2 | 0x04 | PTS | PTS field present |
| 1 | 0x02 | EOF | End of Frame |
| 0 | 0x01 | FID | Frame ID toggle |

### Header Validation (Relaxed)
```rust
// Relaxed validation for cheap cameras:
// - Require: EOH bit (0x80) is set
// - Accept: Length 2-12 regardless of other flags
// Why: Many cheap cameras don't follow UVC spec strictly
```

### Frame Boundary Detection

**For MJPEG:**
- Wait for EOF flag (0x02) in header
- Verify JPEG SOI marker (0xFFD8) at start of frame
- Send complete frame on EOF

**For YUY2 (Uncompressed):**
- Size-based detection: `buffer_size >= expected_frame_size`
- FID toggle is **unreliable** on cheap cameras (toggles mid-frame)
- `drain()` extracts exactly `expected_frame_size` bytes
- **Critical:** Overflow bytes are preserved for next frame (do NOT clear buffer)

### Shared Frame State
```rust
struct SharedFrameState {
    frame_buffer: Vec<u8>,              // Accumulates payload bytes
    last_frame_id: Option<bool>,        // Last FID for boundary detection
    synced: bool,                       // Whether synced to frame boundary
    is_mjpeg: Option<bool>,             // Detected format
    expected_frame_size: usize,         // From descriptor
    validation_warning_count: u32,      // Rate-limits validation logs
}
```

### Output
- Complete frames sent via mpsc channel
- Frame data: raw YUY2 bytes or MJPEG bytes

---

## Stage 5: Frame Validation

**File:** `frame_validation.rs` - `validate_yuy2_frame()`

### What Happens
1. Each assembled YUY2 frame is validated for corruption
2. Validation checks depend on configured strictness level
3. Results are logged (rate-limited) for debugging
4. **Frames are always sent regardless of validation result** (user sees corrupted frames)

### Validation Levels
| Level | Checks Performed | Use Case |
|-------|-----------------|----------|
| `Strict` (default) | Row similarity + size + stride alignment | Detect banding, shearing |
| `Moderate` | Size (±10%) + stride alignment | Detect major corruption |
| `Minimal` | Size only (±100%) | Detect massive mismatches |
| `Off` | None | Best performance |

### Configuration
```bash
# Environment variable (read at startup)
export CLEANSCOPE_FRAME_VALIDATION=strict|moderate|minimal|off
```

### Validation Checks

#### 1. Size Validation (All levels except Off)
```rust
// Minimal: 0.5 ≤ ratio ≤ 2.0 (50% - 200%)
// Moderate/Strict: 0.9 ≤ ratio ≤ 1.1 (90% - 110%)
let size_ratio = actual_size / expected_size;
```

#### 2. Stride Alignment (Moderate, Strict)
```rust
// Check if frame size is aligned to stride (width × 2)
let stride = width * 2;
let stride_aligned = actual_size % stride == 0
    || (actual_size - expected_size).abs() < stride;
```

#### 3. Row Similarity (Strict only)
```rust
// Detects horizontal banding artifacts
// Samples Y channel differences between adjacent rows
// High values (>40) indicate corruption
fn compute_row_similarity(data, stride, height) -> f32 {
    // Sample first 3 rows, every 16th pixel
    // Return average Y-channel difference
}
```

### Threshold Constants
| Constant | Value | Description |
|----------|-------|-------------|
| `STRICT_ROW_DIFF_THRESHOLD` | 40.0 | Max average row difference |
| `MODERATE_SIZE_TOLERANCE` | 1.1 | 10% size tolerance |
| `MINIMAL_SIZE_TOLERANCE` | 2.0 | 100% size tolerance |

### ValidationResult Structure
```rust
pub struct ValidationResult {
    pub valid: bool,                    // Pass/fail
    pub avg_row_diff: Option<f32>,      // Row difference metric (Strict only)
    pub actual_size: usize,             // Actual frame size
    pub expected_size: usize,           // Expected frame size
    pub size_ratio: f32,                // actual / expected
    pub stride_aligned: bool,           // Whether stride is correct
    pub failure_reason: Option<String>, // Human-readable failure reason
}
```

### Logging Behavior
```rust
// Rate-limited to avoid log spam:
// - First 10 warnings logged
// - Then every 100th warning
if warning_count <= 10 || warning_count % 100 == 0 {
    log::warn!("Frame validation failed (#{}) - {}", ...);
}
```

### Output
- Validation result logged for debugging
- Frame **always forwarded** to next stage (no frames dropped)

---

## Stage 6: YUV→RGB Conversion

**File:** `usb.rs` - `stream_frames_yuy2()`, `convert_yuv422_to_rgb()`

### What Happens
1. Receive raw YUY2/UYVY frame from channel
2. Calculate or apply stride
3. Apply user overrides for width/height/stride
4. Convert YUV to RGB using yuvutils-rs
5. Store RGB frame in FrameBuffer

### Stride Handling

**Auto-detect:**
```rust
let actual_stride = frame_size / height;
let actual_width = actual_stride / 2;  // YUY2 = 2 bytes/pixel
```

**User override via STRIDE_OPTIONS:**
```rust
pub const STRIDE_OPTIONS: &[f32] = &[
    2.0,    // Standard YUY2: width × 2 (no padding)
    2.004,  // ~5 extra bytes per row
    2.008,  // ~10 extra bytes per row
    2.016,  // ~20 extra bytes per row
    2.032,  // ~40 extra bytes per row (64-byte alignment)
    2.5,    // 25% extra
    3.0,    // 50% extra
];
// Applied as: stride = width * STRIDE_OPTIONS[stride_index]
```

### Pixel Format Conversion

| PixelFormat | Byte Order | Conversion |
|-------------|------------|------------|
| `Yuyv` (default) | Y0-U-Y1-V | `convert_yuv422_to_rgb(YuvPackedFormat::Yuyv)` |
| `Uyvy` | U-Y0-V-Y1 | `convert_yuv422_to_rgb(YuvPackedFormat::Uyvy)` |
| `Nv12` | Y plane + UV plane | `convert_nv12_to_rgb()` |
| `I420` | Y + U + V planes | `convert_i420_to_rgb()` |
| `Rgb888` | R-G-B | Pass-through |
| `Bgr888` | B-G-R | Swap R↔B |

### YUV Conversion Parameters
```rust
// Using yuvutils-rs library
YuvRange::Limited      // 16-235 for Y, 16-240 for UV
YuvStandardMatrix::Bt601  // Standard video matrix
```

### Output
- RGB frame data (width × height × 3 bytes)
- Stored in `FrameBuffer.frame`
- Raw YUY2 also stored in `FrameBuffer.raw_frame` for debugging

---

## Stage 7: Frontend Display

**File:** `src/App.svelte` - `renderFrame()`

### What Happens
1. Listen for `"frame-ready"` Tauri event
2. Invoke `get_frame()` command to retrieve RGB blob
3. Create `ImageData` from RGB bytes
4. Draw to canvas via `ctx.putImageData()`

### Event Flow
```
Rust: emit("frame-ready", ())
  ↓
Svelte: listen("frame-ready", callback)
  ↓
Svelte: invoke("get_frame") → returns Vec<u8>
  ↓
Svelte: new ImageData(Uint8ClampedArray, width, height)
  ↓
Svelte: ctx.putImageData(imageData, 0, 0)
```

### Frame Info Command
```rust
#[tauri::command]
fn get_frame_info() -> FrameInfo {
    FrameInfo {
        width: buffer.width,
        height: buffer.height,
        format: "RGB",
    }
}
```

---

## User-Configurable Parameters

### AppState Structure
```rust
pub struct AppState {
    pub frame_buffer: Arc<Mutex<FrameBuffer>>,
    pub display_settings: Arc<Mutex<DisplaySettings>>,
    pub streaming_config: Arc<Mutex<StreamingConfig>>,
    pub width_index: Arc<Mutex<Option<usize>>>,
    pub height_index: Arc<Mutex<Option<usize>>>,
    pub stride_index: Arc<Mutex<Option<usize>>>,
    pub usb_stop_flag: Arc<AtomicBool>,
    pub validation_level: ValidationLevel,
}
```

### DisplaySettings
| Field | Type | Control | Description |
|-------|------|---------|-------------|
| `width` | `Option<u32>` | `cycle_width()` | Override auto-detected width |
| `height` | `Option<u32>` | `cycle_height()` | Override auto-detected height |
| `stride` | `Option<u32>` | `cycle_stride()` | Override auto-calculated stride |

### StreamingConfig
| Field | Type | Control | Description |
|-------|------|---------|-------------|
| `skip_mjpeg_detection` | `bool` | `toggle_skip_mjpeg()` | Force YUV path |
| `pixel_format` | `PixelFormat` | `cycle_pixel_format()` | YUV byte order |
| `selected_format_index` | `Option<u8>` | `cycle_video_format()` | Manual format selection |
| `available_formats` | `Vec<DiscoveredFormat>` | (auto) | Formats from camera |
| `restart_requested` | `bool` | (internal) | Trigger stream restart |

### Option Arrays
```rust
pub const WIDTH_OPTIONS: &[u32] = &[1280, 1920, 640, 800, 1024, 960, 720, 1440];
pub const HEIGHT_OPTIONS: &[u32] = &[720, 1080, 480, 600, 768, 540, 960, 800];
pub const STRIDE_OPTIONS: &[f32] = &[2.0, 2.004, 2.008, 2.016, 2.032, 2.5, 3.0];
```

---

## Data Flow Summary

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Data Flow Between Stages                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  USB Descriptors                                                            │
│       ↓                                                                     │
│  [Stage 1] → Vec<DiscoveredFormat> → StreamingConfig.available_formats      │
│       ↓                                                                     │
│  [Stage 2] → UvcNegotiatedParams {endpoint, width, height, format_index}    │
│       ↓                                                                     │
│  [Stage 3] → mpsc::channel<Vec<u8>> (frame receiver)                        │
│       ↓                                                                     │
│  [Stage 4] → Complete frames (raw YUY2 or MJPEG bytes)                      │
│       ↓                                                                     │
│  [Stage 5] → ValidationResult logged, frame forwarded unchanged             │
│       ↓                                                                     │
│  [Stage 6] → RGB frame → FrameBuffer {frame, width, height, timestamp}      │
│       ↓                                                                     │
│  [Stage 7] → emit("frame-ready") → Canvas display                           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Debugging Reference

### Common Issues by Stage

| Stage | Issue | Symptom | Fix |
|-------|-------|---------|-----|
| 2 | Wrong format selected | No video or garbage | Try `cycle_video_format()` |
| 4 | Headers treated as pixels | Shifted rows, periodic corruption | Check `validate_uvc_header()` |
| 4 | Buffer cleared after drain | Interlacing/tearing | Remove buffer.clear() |
| 5 | High row diff | Banding artifacts | Check frame boundary timing |
| 6 | Wrong pixel format | Green/magenta tint | `cycle_pixel_format()` |
| 6 | Wrong stride | Diagonal shearing | `cycle_stride()` |

### Useful Log Filters
```bash
# Frame assembly
adb logcat -s RustStdoutStderr:* | grep -E "frame|Frame|YUY2|MJPEG"

# Validation warnings
adb logcat -s RustStdoutStderr:* | grep "validation"

# Stride/resolution
adb logcat -s RustStdoutStderr:* | grep -E "stride|resolution|width|height"

# Header issues
adb logcat -s RustStdoutStderr:* | grep -E "header|SUSPICIOUS"
```

### Validation Metrics Interpretation

| Metric | Good Value | Bad Value | Indicates |
|--------|------------|-----------|-----------|
| `avg_row_diff` | < 20 | > 40 | Banding/corruption |
| `size_ratio` | 0.98-1.02 | < 0.9 or > 1.1 | Frame size issue |
| `stride_aligned` | true | false | Padding mismatch |
