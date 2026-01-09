---
created: 2025-01-08
modified: 2025-01-08
reviewed: 2025-01-08
name: USB Camera Analysis
description: Comprehensive USB camera and UVC device analysis for debugging video streaming issues. Use when diagnosing camera connectivity, format negotiation, or control capabilities.
allowed-tools: Bash, Read, Grep, Glob
---

# USB Camera Analysis

Expert knowledge for analyzing USB cameras and UVC devices to debug video streaming issues and understand device capabilities.

## When to Use

- Debugging camera connectivity issues
- Understanding available video formats and resolutions
- Discovering UVC control capabilities (brightness, exposure, etc.)
- Verifying device descriptors match expected values
- Configuring CleanScope for a new endoscope device

## Quick Device Identification

### macOS - List Available Cameras

```bash
# List AVFoundation video devices
ffmpeg -f avfoundation -list_devices true -i "" 2>&1 | grep -A 20 "video devices"

# Get USB device details
ioreg -p IOUSB -w 0 | grep -i "camera\|video\|endoscope"
```

### Linux - List V4L2 Devices

```bash
# List video devices
v4l2-ctl --list-devices

# Get device capabilities
v4l2-ctl -d /dev/video0 --all
```

## Video Stream Analysis with ffprobe

### Basic Probe

```bash
# macOS - probe device 0
ffprobe -f avfoundation -framerate 30 -video_size 640x480 -i "0" \
  -show_format -show_streams 2>&1

# Linux
ffprobe -f v4l2 -i /dev/video0 -show_format -show_streams 2>&1
```

### Key Information to Extract

| Field | Meaning |
|-------|---------|
| `pix_fmt` | Pixel format (uyvy422, yuyv422, nv12, mjpeg) |
| `width/height` | Resolution |
| `r_frame_rate` | Actual frame rate |
| `codec_tag_string` | FourCC code (YUYV, UYVY, MJPG) |

### Supported Formats

```bash
# macOS - see what formats ffmpeg can use
ffprobe -f avfoundation -i "0" 2>&1 | grep "Supported pixel formats"

# Linux
v4l2-ctl -d /dev/video0 --list-formats-ext
```

## USB Descriptor Analysis

### Get VID/PID and Basic Info

```bash
# macOS
ioreg -r -c IOUSBHostDevice -n "HD camera" -l | grep -E "idVendor|idProduct|USB Product Name"

# Linux
lsusb -v -d VID:PID | head -50
```

### Parse UVC Descriptors with Python

```python
import usb.core

# Find device by VID:PID
dev = usb.core.find(idVendor=0x349c, idProduct=0x0411)

print(f"Device: {dev.product}")
print(f"Manufacturer: {dev.manufacturer}")

# List interfaces
for cfg in dev:
    for intf in cfg:
        class_name = {14: "Video"}.get(intf.bInterfaceClass, f"Class {intf.bInterfaceClass}")
        subclass = {1: "Control", 2: "Streaming"}.get(intf.bInterfaceSubClass, "")
        print(f"Interface {intf.bInterfaceNumber}: {class_name} {subclass}")
```

## UVC Control Discovery

### Using uvc-util (macOS)

```bash
# List UVC devices
uvc-util -d

# List controls for device 0
uvc-util -I 0 -c

# Show all control details
uvc-util -I 0 -S '*'

# Get specific control value
uvc-util -I 0 -g brightness

# Set control value
uvc-util -I 0 -s brightness=0.5
```

### Control Bitmap Interpretation

UVC controls are advertised via bitmaps in descriptors:

**Camera Terminal Controls** (byte 15-17 of INPUT_TERMINAL):
| Bit | Control |
|-----|---------|
| 0 | Scanning Mode |
| 1 | Auto-Exposure Mode |
| 2 | Auto-Exposure Priority |
| 3 | Exposure Time (Absolute) |
| 5 | Focus (Absolute) |
| 9 | Zoom (Absolute) |
| 11 | Pan/Tilt (Absolute) |
| 16 | Focus Auto |
| 17 | Privacy |

**Processing Unit Controls** (byte 8+ of PROCESSING_UNIT):
| Bit | Control |
|-----|---------|
| 0 | Brightness |
| 1 | Contrast |
| 2 | Hue |
| 3 | Saturation |
| 4 | Sharpness |
| 5 | Gamma |
| 6 | White Balance Temperature |
| 8 | Backlight Compensation |
| 9 | Gain |
| 10 | Power Line Frequency |

**Bitmap = 0x000000**: Device has NO controls (common for cheap endoscopes)

## Common Device Patterns

### Cheap USB Endoscopes

Typical characteristics:
- VID: 0x349c (Generic)
- UVC 1.00
- **No UVC controls** (fixed settings)
- YUY2/UYVY format only (no MJPEG)
- Single format with multiple resolutions

### Quality Webcams

Typical characteristics:
- Brand-specific VID (Logitech: 0x046d)
- UVC 1.1 or 1.5
- Full control support (brightness, exposure, focus)
- MJPEG + YUY2 formats
- Auto-focus, auto-exposure

## Troubleshooting

| Symptom | Likely Cause | Solution |
|---------|--------------|----------|
| Device not detected | Driver/permission issue | Check USB permission (Android), kext (macOS) |
| Wrong pixel format | Format negotiation failed | Try different format index |
| No UVC controls found | Cheap camera with fixed settings | Use software post-processing |
| Green/magenta tint | YUYV vs UYVY mismatch | Toggle pixel format in app |
| Low frame rate | Wrong resolution/format | Try lower resolution |

## CleanScope Integration

After analyzing a camera:

1. **Update device_filter.xml** with VID/PID:
   ```xml
   <usb-device vendor-id="13468" product-id="1041" />
   ```

2. **Set default pixel format** based on probe results:
   - If camera prefers UYVY, set `PixelFormat::Uyvy` as default
   - If MJPEG available, enable MJPEG detection

3. **Note control availability** in device documentation:
   - If no controls: Document that brightness/contrast must be software-adjusted
   - If controls available: Consider adding UI for hardware adjustments

## See Also

- [REFERENCE.md](REFERENCE.md) - Detailed UVC descriptor codes and pyusb scripts
- `/usb:probe` - Quick camera probe command
- `/usb:descriptors` - Full descriptor analysis
- `/usb:controls` - UVC control discovery
- `/usb:analyze` - Comprehensive analysis
