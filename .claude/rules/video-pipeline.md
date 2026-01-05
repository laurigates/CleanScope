# Video Pipeline Debugging

## Pipeline Overview

```
Camera → USB Isochronous → UVC Headers → Frame Assembly → YUV→RGB → Canvas
```

Each stage can introduce artifacts. Debug systematically from left to right.

## Critical Implementation Details

### Frame Assembly (libusb_android.rs)

**Frame boundary detection for YUY2:**
- Uses size-based detection: `buffer_size >= expected_frame_size`
- FID toggle is unreliable on cheap cameras (toggles mid-frame)
- After `drain()`, do NOT `clear()` the buffer - remaining bytes are next frame's data

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

## Common Pitfalls

- Clearing frame buffer after drain() → interlacing
- Strict header validation → rejected headers become pixel data
- Ignoring stride_index in streaming code → UI button does nothing
- Not setting buffer.width/height → frontend uses wrong dimensions
