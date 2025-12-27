# UVC Camera Streaming

## Overview
USB Video Class (UVC) protocol implementation for streaming video from USB cameras and endoscopes.

## When to Use
- Implementing USB camera streaming
- Debugging frame reception issues
- Understanding UVC protocol negotiation
- Parsing MJPEG frames from UVC devices

## Key Concepts

### UVC Protocol Overview
UVC devices use a standardized protocol:
1. **Probe/Commit Negotiation**: Host and device agree on video format
2. **Control Transfers**: Configure device parameters
3. **Bulk/Isochronous Transfers**: Stream video data

### Control Transfer Structure
```rust
// UVC control request types
const UVC_SET_CUR: u8 = 0x01;
const UVC_GET_CUR: u8 = 0x81;
const UVC_GET_MIN: u8 = 0x82;
const UVC_GET_MAX: u8 = 0x83;
const UVC_GET_DEF: u8 = 0x87;

// Control selectors
const VS_PROBE_CONTROL: u16 = 0x0100;
const VS_COMMIT_CONTROL: u16 = 0x0200;
```

### UvcStreamControl Structure
The probe/commit structure (26-34 bytes depending on UVC version):
```rust
#[repr(C, packed)]
struct UvcStreamControl {
    bmHint: u16,              // Negotiation hints
    bFormatIndex: u8,         // Video format index
    bFrameIndex: u8,          // Frame size index
    dwFrameInterval: u32,     // Frame interval in 100ns units
    wKeyFrameRate: u16,       // Key frame rate
    wPFrameRate: u16,         // P-frame rate
    wCompQuality: u16,        // Compression quality
    wCompWindowSize: u16,     // Compression window
    wDelay: u16,              // Latency in frames
    dwMaxVideoFrameSize: u32, // Max frame size
    dwMaxPayloadTransferSize: u32, // Max transfer size
    // UVC 1.1+ additional fields...
}
```

### MJPEG Frame Detection
```rust
const JPEG_SOI: [u8; 2] = [0xFF, 0xD8];  // Start of Image
const JPEG_EOI: [u8; 2] = [0xFF, 0xD9];  // End of Image

fn is_complete_frame(data: &[u8]) -> bool {
    data.len() >= 4
        && data[0..2] == JPEG_SOI
        && data[data.len()-2..] == JPEG_EOI
}
```

### UVC Payload Header
First 12 bytes of each transfer contain header info:
```rust
struct UvcPayloadHeader {
    bHeaderLength: u8,    // Header length
    bmHeaderInfo: u8,     // Frame flags
    // Bit 0: Frame ID (toggles each frame)
    // Bit 1: End of Frame
    // Bit 2: Presentation Time present
    // etc.
}

fn is_end_of_frame(header: u8) -> bool {
    header & 0x02 != 0
}
```

## Common Patterns

### Probe/Commit Negotiation
```rust
// 1. GET_DEF to get default settings
let default_ctrl = control_transfer(UVC_GET_DEF, VS_PROBE_CONTROL)?;

// 2. SET_CUR with desired settings
let mut probe = default_ctrl;
probe.bFormatIndex = 1;  // MJPEG format
probe.bFrameIndex = 1;   // First resolution
control_transfer(UVC_SET_CUR, VS_PROBE_CONTROL, &probe)?;

// 3. GET_CUR to see device's response
let negotiated = control_transfer(UVC_GET_CUR, VS_PROBE_CONTROL)?;

// 4. SET_CUR to commit
control_transfer(UVC_SET_CUR, VS_COMMIT_CONTROL, &negotiated)?;
```

### Frame Assembly from Bulk Transfers
```rust
fn assemble_frame(transfers: &[Vec<u8>]) -> Option<Vec<u8>> {
    let mut frame = Vec::new();

    for transfer in transfers {
        if transfer.len() < 12 {
            continue;
        }

        let header = transfer[1];
        let payload = &transfer[12..];

        frame.extend_from_slice(payload);

        if is_end_of_frame(header) {
            if is_complete_frame(&frame) {
                return Some(frame);
            }
            frame.clear();
        }
    }
    None
}
```

### Polling Pattern for Frame Streaming
```rust
loop {
    match bulk_transfer(endpoint, &mut buffer, timeout) {
        Ok(bytes_read) => {
            process_uvc_data(&buffer[..bytes_read]);
        }
        Err(rusb::Error::Timeout) => {
            // No data available, continue polling
            continue;
        }
        Err(e) => {
            log::error!("Bulk transfer error: {}", e);
            break;
        }
    }
}
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| No frames received | Check endpoint address (usually 0x81 or 0x82) |
| Incomplete frames | Increase buffer size or timeout |
| Corrupted JPEG | Verify SOI/EOI markers, check for dropped transfers |
| `LIBUSB_ERROR_PIPE` | Device rejected control transfer, check request format |
| `LIBUSB_ERROR_TIMEOUT` | Increase timeout or check device connection |
| Wrong resolution | Check bFrameIndex in probe negotiation |
| Frame ID not toggling | Device may not support frame sync |

## Common UVC Error Codes

| Error | Meaning |
|-------|---------|
| `-1` (LIBUSB_ERROR_IO) | I/O error during transfer |
| `-4` (LIBUSB_ERROR_NO_DEVICE) | Device disconnected |
| `-6` (LIBUSB_ERROR_BUSY) | Resource busy (interface not claimed) |
| `-7` (LIBUSB_ERROR_TIMEOUT) | Transfer timed out |
| `-9` (LIBUSB_ERROR_PIPE) | Control request not supported |

## References
- [USB Video Class 1.5 Specification](https://www.usb.org/document-library/video-class-v15-document-set)
- [libusb Documentation](https://libusb.info/api-1.0/)
- [MJPEG Format](https://en.wikipedia.org/wiki/Motion_JPEG)
