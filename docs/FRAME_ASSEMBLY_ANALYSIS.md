# Frame Assembly Analysis

Analysis of the CleanScope UVC frame assembly implementation against industry best practices and common issues.

## Executive Summary

The current implementation is generally well-designed with:
- URB ordering via sequence numbers (solves out-of-order callback issue)
- Size-based frame boundary detection for YUY2 (reliable for cheap cameras)
- Relaxed UVC header validation (tolerates non-compliant devices)
- Proper overflow byte preservation

However, several potential row ordering issues were identified that could cause the symptoms described.

---

## Architecture Overview

### Frame Assembly Pipeline

```
Isochronous Transfer Complete
       ↓
extract_urb_payloads()          [libusb_android.rs:1638]
  - Strip UVC headers
  - Extract FID/EOF flags
  - Record payload metadata
       ↓
Store in pending_urbs BTreeMap  [libusb_android.rs:1566]
  - Keyed by sequence number
       ↓
process_pending_urbs_in_order() [libusb_android.rs:1805]
  - Process URBs in sequence
       ↓
process_urb_payload_in_order()  [libusb_android.rs:1721]
  - Accumulate to frame_buffer
  - Detect frame boundaries
       ↓
emit_yuy2_frame()               [libusb_android.rs:1193]
  - Drain expected_size bytes
  - Validate frame
  - Send to frontend
```

### Key Data Structures

| Structure | Location | Purpose |
|-----------|----------|---------|
| `SharedFrameState` | libusb_android.rs:1099 | Accumulation buffer + pending URBs |
| `UrbPayload` | libusb_android.rs:1612 | Extracted payload from one URB |
| `PacketMeta` | libusb_android.rs:1619 | Per-packet flags (FID, EOF, error) |
| `FrameAssembler` | frame_assembler.rs:48 | Standalone assembler for testing |

---

## Potential Row Ordering Issues

### Issue 1: Sequence Counter Overflow Edge Case

**Location:** `libusb_android.rs:1544`

```rust
let sequence = context.sequence_counter.fetch_add(1, Ordering::SeqCst);
```

**Analysis:** The sequence counter is `AtomicU64` which won't overflow in practice, but `SeqCst` ordering is heavier than needed. More importantly, if the sequence wraps (after ~18 quintillion frames), the `BTreeMap` ordering would break.

**Risk:** Very Low (practically impossible to reach)

**Recommendation:** No action needed.

---

### Issue 2: FID Toggle Not Used for YUY2

**Location:** `libusb_android.rs:1768`

```rust
// For YUY2: FID toggle is unreliable, don't use for frame boundaries
state.synced = true;
```

**Analysis:** This is correct behavior. Research confirms:
> "End of frame detection is better implemented by checking the EOF bit (FID bit toggling is delayed by one frame compared to the EOF bit), but some devices don't set the bit at end of frame" — Linux UVC driver

The code properly uses size-based detection for YUY2 instead.

**Risk:** None (correct design decision)

---

### Issue 3: Potential Data Race in Multi-Transfer Callbacks

**Location:** `libusb_android.rs:1557-1563`

```rust
let mut state = match context.shared_state.lock() {
    Ok(guard) => guard,
    Err(poisoned) => {
        log::error!("Shared state mutex poisoned, recovering");
        poisoned.into_inner()
    }
};
```

**Analysis:** The code correctly uses a mutex for shared state. However, if multiple transfers complete simultaneously (from different threads), they could interleave their sequence numbers despite the atomic counter.

Consider this race:
1. Transfer A: gets sequence 5, starts locking
2. Transfer B: gets sequence 6, acquires lock first
3. Transfer B: processes sequence 6 (but 5 not yet in pending_urbs)
4. Transfer A: acquires lock, processes sequence 5

The `BTreeMap` and `next_expected_sequence` mechanism should handle this correctly by deferring out-of-order URBs.

**Risk:** Low (correctly mitigated by pending_urbs queue)

---

### Issue 4: UVC Header False Positives in YUY2 Data

**Location:** `frame_assembler.rs:342-361`

```rust
pub fn validate_uvc_header(data: &[u8]) -> Option<usize> {
    // EOH (End of Header) bit MUST be set for valid headers
    if (header_flags & 0x80) == 0 {
        return None;
    }
    // Basic sanity check on length
    if !(2..=12).contains(&header_len) || header_len > data.len() {
        return None;
    }
    Some(header_len)
}
```

**Analysis:** YUY2 pixel data can accidentally match header patterns:
- Byte pattern `[0x02, 0x80, ...]` could be valid YUY2 (Y=2, U=128)
- This would cause 2 bytes to be stripped from payload data

The test at `libusb_android.rs:1913-1921` confirms this is known:
```rust
// YUY2 data that might look like a header:
// Y=2 (could be header_len=2), U=128 (has EOH bit set)
// This will actually be accepted as a valid 2-byte header
```

**Risk:** Medium - This could cause periodic 2-12 byte shifts in row data

**Root Cause of Row Shifting:** If a YUY2 payload accidentally matches a UVC header pattern, the "header" bytes are stripped, causing:
- All subsequent bytes in that packet to shift left by header_len
- Row misalignment by header_len bytes
- Diagonal shearing artifacts

**Recommendation:** After format detection confirms YUY2, skip header validation entirely for payloads. The UVC header appears only at the start of each isochronous packet, not mid-stream.

---

### Issue 5: Zero-Fill Detection May Skip Valid Dark Data

**Location:** `libusb_android.rs:1698-1702`

```rust
// Skip zero-filled payloads
let payload_to_add = if payload.len() > 8 && payload[0..8].iter().all(|&b| b == 0) {
    &[]
} else {
    payload
};
```

**Analysis:** This heuristic could incorrectly skip valid video data if:
- The camera is pointed at a completely black scene
- The first 8 bytes of a row happen to be Y=0, U=0, Y=0, V=0, Y=0, U=0, Y=0, V=0

YUY2 black would be approximately `[16, 128, 16, 128, ...]` (ITU-R BT.601), not pure zeros. However, some cameras use 0 for black.

**Risk:** Low (unlikely in practice)

**Recommendation:** Consider reducing the zero-check to only skip packets that are entirely zero, or check for more than 8 bytes.

---

### Issue 6: Expected Frame Size Mismatch

**Location:** `libusb_android.rs:1789`

```rust
// For YUY2: Check if buffer has reached expected frame size
if !is_mjpeg && state.frame_buffer.len() >= state.expected_frame_size {
    emit_yuy2_frame(state, context);
}
```

**Analysis:** If `expected_frame_size` doesn't match the actual camera output:
- Too small: Frames split mid-row, causing interlacing
- Too large: Multiple frames concatenated, causing overlay/ghosting

The expected size comes from UVC descriptor parsing and is passed to `IsochronousStream::new()`.

**Risk:** Medium if descriptor parsing is incorrect

**Recommendation:** Add runtime detection similar to `FrameAssembler::handle_yuy2_fid_toggle()`:
```rust
// Auto-correct expected_frame_size if significantly different
let size_ratio = buffer_size as f32 / self.expected_frame_size as f32;
if !(0.7..=1.5).contains(&size_ratio) {
    // Recalculate expected size
}
```

---

### Issue 7: Packet Processing Order Within URB

**Location:** `libusb_android.rs:1729-1800`

```rust
for pkt in &payload.packets {
    // Process each packet's metadata
    // ...
    if pkt.payload_len > 0 {
        let payload_slice = &payload.data[data_offset..data_offset + pkt.payload_len];
        state.frame_buffer.extend_from_slice(payload_slice);
    }
    data_offset += pkt.payload_len;
}
```

**Analysis:** Packets within a single URB are processed in order (correct). The `data_offset` tracker ensures we extract the right slice from the concatenated payload data.

**Risk:** None (correctly implemented)

---

## Comparison with Best Practices

### Linux UVC Driver Approach

The Linux kernel UVC driver (`uvc_video.c`) uses:
1. **FID toggle** for primary frame boundary detection
2. **EOF bit** as secondary/optimization
3. **Buffer state machine** (QUEUED → ACTIVE → READY)

CleanScope differs by:
1. Using **size-based** detection for YUY2 (more reliable for cheap cameras)
2. Using **FID toggle** only for MJPEG
3. Using **sequence-numbered URB queue** (handles async callback ordering)

### ESP32 UVC Implementation Issues

The ESP-IDF UVC implementation had a similar bug:
> "The Frame Callback has packet lengths around 1 to 2K bytes when it should be closer to 61K bytes...the code appears incorrect for building up the full packet, resulting in scrambled data."

CleanScope avoids this by:
- Processing packets individually, not assuming full frames per callback
- Using configurable `packets_per_transfer` (default 32)
- Properly accumulating across multiple URBs

---

## Diagnostic Checklist

When debugging row ordering issues:

1. **Check frame size logging:**
   ```bash
   adb logcat -s RustStdoutStderr:* | grep "Complete YUY2 frame"
   ```
   Verify `expected_size` matches actual frame size.

2. **Check for header false positives:**
   ```bash
   adb logcat -s RustStdoutStderr:* | grep -E "validate_uvc_header|header_len"
   ```
   Look for unexpected header detections mid-frame.

3. **Check URB ordering:**
   ```bash
   adb logcat -s RustStdoutStderr:* | grep "URB out of order"
   ```
   Frequent out-of-order messages indicate callback timing issues.

4. **Check validation warnings:**
   ```bash
   adb logcat -s RustStdoutStderr:* | grep "validation failed"
   ```
   Look at `avg_row_diff` and `size_ratio` values.

5. **Capture raw frames:**
   Use the UI capture button to save raw YUY2 data for offline analysis.

---

## Recommendations

### High Priority

1. **Skip header validation after YUY2 format detection**

   Once `is_mjpeg == Some(false)`, treat all packet data as pure payload:
   ```rust
   let (is_uvc_header, header_len) = if state.is_mjpeg == Some(false) {
       (false, 0)  // Skip header detection for YUY2
   } else {
       let validated = validate_uvc_header(pkt_data);
       (validated.is_some(), validated.unwrap_or(0))
   };
   ```

2. **Add expected_frame_size auto-correction**

   Detect when actual frame sizes consistently differ from expected and adjust.

### Medium Priority

3. **Improve zero-fill detection**

   Check more bytes or use a different heuristic to avoid skipping valid dark frames.

4. **Add frame boundary logging**

   Log when frames are emitted with their trigger (size-based, FID, EOF) for debugging.

### Low Priority

5. **Consider double-buffering**

   Emit frames to a swap buffer to reduce lock contention.

---

## Related Files

| File | Purpose |
|------|---------|
| `src-tauri/src/libusb_android.rs` | USB transfer handling, frame assembly |
| `src-tauri/src/frame_assembler.rs` | Standalone assembler (testable) |
| `src-tauri/src/frame_validation.rs` | Corruption detection |
| `src-tauri/src/usb.rs` | YUV→RGB conversion, display settings |
| `docs/VIDEO_PIPELINE.md` | Full pipeline documentation |

## References

- [Microsoft YUV Formats](https://learn.microsoft.com/en-us/windows/win32/medfound/recommended-8-bit-yuv-formats-for-video-rendering)
- [Linux UVC Driver FAQ](https://www.ideasonboard.org/uvc/faq/)
- [UVC Frame Detector](https://github.com/Vaultmicro/uvc_frame_detector)
- [Frame Corruption in Linux](https://www.e-consystems.com/blog/camera/products/handling-frame-corruption-in-linux-for-high-resolution-imaging/)
- [Intra-frame Corruption Workaround](https://github.com/alanz2015/UVC-revised-for-suppress-intraframe-data-corruptioin)
