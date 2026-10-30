# Video Pipeline Debugging

## Pipeline Overview

```
Camera → USB Isochronous → UVC Headers → Frame Assembly → Frame Validation → YUV→RGB → Canvas
  (1)         (2)              (3)            (4)              (5)            (6)        (7)
```

Each stage can introduce artifacts. Debug systematically from left to right.

**Full documentation:** See `docs/VIDEO_PIPELINE.md` for complete technical reference.

## Critical Implementation Details

### Frame Assembly (libusb_android.rs)

**Frame boundary detection for YUY2:**
- Uses size-based detection: `buffer_size >= expected_frame_size`
- FID toggle is unreliable on cheap cameras (toggles mid-frame), so it is trusted only after two consecutive FID periods equal exactly one frame (`update_fid_period_matches`). A trusted toggle with a partial buffer drops that partial frame to re-align.
- After `drain()`, do NOT `clear()` the buffer - remaining bytes are next frame's data
- Constant non-zero "overflow bytes preserved" means the cut is off the camera's frame boundary, not that headers leaked in. Every payload here is a multiple of 12 bytes, so a multiple-of-12 overflow proves nothing about headers.

**Isochronous transfers (Android usbfs):**
- usbfs allocates each URB buffer as one contiguous kernel block. 32 × 3072-byte packets (~96 KB) intermittently failed with `errno=12` (ENOMEM) on Android 16, which libusb reports only as `LIBUSB_ERROR_IO` (-1) on submit. Keep URBs small (`ISO_CONFIG`: 16 transfers × 8 packets) and add transfers, not packets, for throughput.
- libusb's own log level is set to warning at init, so the kernel errno appears in logcat as `libusb: error [submit_iso_transfer] submiturb failed, errno=N`. Look there before theorising about a `-1`.
- `libusb_close()` leaves a wrapped Android fd open, and usbfs keeps interface claims until that fd closes. `LibusbDeviceHandle` therefore releases its claims on drop; without that, every reconnect fails with `libusb_claim_interface failed: -6` (EBUSY).

**UVC header parsing:**
- Every isochronous packet starts with 2-12 byte header
- Header byte 0: length, byte 1: flags (EOH=0x80, EOF=0x02, FID=0x01)
- If validation fails, header bytes become pixel data → corruption
- Use relaxed validation: trust length if EOH is set

### YUV Conversion (usb.rs)

**Stride matters:**
- Stride = bytes per row (may include padding)
- For YUY2: stride = width × 2 (no padding) or more
- Auto-detect: `actual_stride = frame_size / height`
- UI override via stride_index → STRIDE_OPTIONS multiplier

**Format matters:**
- YUYV: Y0-U-Y1-V (common)
- UYVY: U-Y0-V-Y1 (less common)
- Wrong format → green/magenta tint

## Debugging Checklist

1. **Check frame size:** Should be width × height × 2 for YUY2
2. **Check for "SUSPICIOUS" logs:** Headers being treated as data
3. **Check overflow bytes:** Should be preserved, not discarded
4. **Try stride adjustment:** Use UI buttons to find correct value
5. **Try YUV format toggle:** Switch YUYV ↔ UYVY if colors are wrong

## Frame Validation (libusb_android.rs + frame_validation.rs)

**What it does:**
- Validates each assembled YUY2 frame for corruption artifacts
- Configurable via `CLEANSCOPE_FRAME_VALIDATION` env var (strict/moderate/minimal/off)
- Logs warnings (rate-limited) but **never drops frames** - user sees everything

**Validation checks by level:**
| Level | Size Check | Stride Alignment | Row Similarity |
|-------|-----------|------------------|----------------|
| Strict | ±10% | Yes | Yes (threshold: 40) |
| Moderate | ±10% | Yes | No |
| Minimal | ±100% | No | No |
| Off | No | No | No |

**Key metrics:**
- `avg_row_diff` > 40 → banding/corruption detected
- `size_ratio` outside 0.9-1.1 → frame size issue
- `stride_aligned` = false → padding mismatch

**Code path:**
```
process_iso_packets() → drain frame → validate_yuy2_frame() → log if invalid → send frame
```

## Common Pitfalls

- Clearing frame buffer after drain() → interlacing
- Strict header validation → rejected headers become pixel data
- Ignoring stride_index in streaming code → UI button does nothing
- Not setting buffer.width/height → frontend uses wrong dimensions
- High validation warnings → check frame boundary detection, not conversion
- Reinstalling the app (`just wifi-deploy`) can drop the USB permission grant ("User has not given … permission to access device"). Unplug and replug the camera to re-trigger `USB_DEVICE_ATTACHED`.
- Android-only code (`libusb_android.rs` and its `#[cfg(test)]` module) is not compiled by desktop `cargo test`/`cargo clippy`. Only an Android build (`just wifi-deploy`) checks it.
