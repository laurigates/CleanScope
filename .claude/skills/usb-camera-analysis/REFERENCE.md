# USB Camera Analysis Reference

Detailed technical reference for UVC descriptors, control bitmaps, format GUIDs, and analysis scripts.

## UVC Descriptor Types

### Interface Descriptor Subtypes (Video Control)

| Value | Name | Description |
|-------|------|-------------|
| 0x01 | VC_HEADER | Video control interface header |
| 0x02 | VC_INPUT_TERMINAL | Camera or other input |
| 0x03 | VC_OUTPUT_TERMINAL | USB streaming output |
| 0x04 | VC_SELECTOR_UNIT | Input selector |
| 0x05 | VC_PROCESSING_UNIT | Image processing controls |
| 0x06 | VC_EXTENSION_UNIT | Vendor-specific controls |

### Interface Descriptor Subtypes (Video Streaming)

| Value | Name | Description |
|-------|------|-------------|
| 0x01 | VS_INPUT_HEADER | Streaming interface header |
| 0x02 | VS_OUTPUT_HEADER | Output header (rare) |
| 0x03 | VS_STILL_IMAGE_FRAME | Still image capture |
| 0x04 | VS_FORMAT_UNCOMPRESSED | Uncompressed format (YUV) |
| 0x05 | VS_FRAME_UNCOMPRESSED | Uncompressed frame size |
| 0x06 | VS_FORMAT_MJPEG | MJPEG format |
| 0x07 | VS_FRAME_MJPEG | MJPEG frame size |

### Terminal Types

| Value | Name |
|-------|------|
| 0x0101 | TT_STREAMING (USB streaming) |
| 0x0201 | ITT_CAMERA (Camera sensor) |
| 0x0202 | ITT_MEDIA_TRANSPORT_INPUT |

## Format GUIDs

### Uncompressed Formats

```
YUY2:  59 55 59 32 00 00 10 00 80 00 00 AA 00 38 9B 71
UYVY:  55 59 56 59 00 00 10 00 80 00 00 AA 00 38 9B 71
NV12:  4E 56 31 32 00 00 10 00 80 00 00 AA 00 38 9B 71
I420:  49 34 32 30 00 00 10 00 80 00 00 AA 00 38 9B 71
YV12:  59 56 31 32 00 00 10 00 80 00 00 AA 00 38 9B 71
```

### RGB Formats

```
RGB24: 7D EB 36 E4 4F 52 CE 11 9F 53 00 20 AF 0B A7 70
BGR24: E4 36 EB 7D 52 4F 11 CE 9F 53 00 20 AF 0B A7 70
```

## Camera Terminal Control Bitmap

Bytes 15-17 of INPUT_TERMINAL descriptor (bControlSize=3):

| Bit | D0 | Control Name |
|-----|-----|--------------|
| 0 | 0x000001 | Scanning Mode |
| 1 | 0x000002 | Auto-Exposure Mode |
| 2 | 0x000004 | Auto-Exposure Priority |
| 3 | 0x000008 | Exposure Time (Absolute) |
| 4 | 0x000010 | Exposure Time (Relative) |
| 5 | 0x000020 | Focus (Absolute) |
| 6 | 0x000040 | Focus (Relative) |
| 7 | 0x000080 | Iris (Absolute) |
| 8 | 0x000100 | Iris (Relative) |
| 9 | 0x000200 | Zoom (Absolute) |
| 10 | 0x000400 | Zoom (Relative) |
| 11 | 0x000800 | Pan/Tilt (Absolute) |
| 12 | 0x001000 | Pan/Tilt (Relative) |
| 13 | 0x002000 | Roll (Absolute) |
| 14 | 0x004000 | Roll (Relative) |
| 15 | 0x008000 | Reserved |
| 16 | 0x010000 | Focus Auto |
| 17 | 0x020000 | Privacy |

## Processing Unit Control Bitmap

Bytes 8+ of PROCESSING_UNIT descriptor:

| Bit | D0 | Control Name |
|-----|-----|--------------|
| 0 | 0x0001 | Brightness |
| 1 | 0x0002 | Contrast |
| 2 | 0x0004 | Hue |
| 3 | 0x0008 | Saturation |
| 4 | 0x0010 | Sharpness |
| 5 | 0x0020 | Gamma |
| 6 | 0x0040 | White Balance Temperature |
| 7 | 0x0080 | White Balance Component |
| 8 | 0x0100 | Backlight Compensation |
| 9 | 0x0200 | Gain |
| 10 | 0x0400 | Power Line Frequency |
| 11 | 0x0800 | Hue Auto |
| 12 | 0x1000 | White Balance Temperature Auto |
| 13 | 0x2000 | White Balance Component Auto |
| 14 | 0x4000 | Digital Multiplier |
| 15 | 0x8000 | Digital Multiplier Limit |
| 16 | 0x010000 | Analog Video Standard |
| 17 | 0x020000 | Analog Video Lock Status |
| 18 | 0x040000 | Contrast Auto |

## Complete Python Analysis Script

```python
#!/usr/bin/env python3
"""
USB Camera Analysis Script
Parses UVC descriptors to discover formats, resolutions, and controls.
Usage: python usb_camera_analysis.py [VID:PID]
"""

import sys
import usb.core

# UVC descriptor constants
CS_INTERFACE = 0x24
VC_HEADER = 0x01
VC_INPUT_TERMINAL = 0x02
VC_OUTPUT_TERMINAL = 0x03
VC_PROCESSING_UNIT = 0x05
VS_INPUT_HEADER = 0x01
VS_FORMAT_UNCOMPRESSED = 0x04
VS_FRAME_UNCOMPRESSED = 0x05
VS_FORMAT_MJPEG = 0x06
VS_FRAME_MJPEG = 0x07

# Format GUIDs
GUID_YUY2 = bytes([0x59, 0x55, 0x59, 0x32, 0x00, 0x00, 0x10, 0x00,
                   0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71])
GUID_UYVY = bytes([0x55, 0x59, 0x56, 0x59, 0x00, 0x00, 0x10, 0x00,
                   0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71])
GUID_NV12 = bytes([0x4E, 0x56, 0x31, 0x32, 0x00, 0x00, 0x10, 0x00,
                   0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71])

# Camera terminal control names
CAMERA_CONTROLS = [
    "Scanning Mode", "Auto-Exposure Mode", "Auto-Exposure Priority",
    "Exposure Time (Abs)", "Exposure Time (Rel)", "Focus (Abs)",
    "Focus (Rel)", "Iris (Abs)", "Iris (Rel)", "Zoom (Abs)",
    "Zoom (Rel)", "Pan/Tilt (Abs)", "Pan/Tilt (Rel)", "Roll (Abs)",
    "Roll (Rel)", "Reserved", "Focus Auto", "Privacy"
]

# Processing unit control names
PROCESSING_CONTROLS = [
    "Brightness", "Contrast", "Hue", "Saturation", "Sharpness",
    "Gamma", "White Balance Temp", "White Balance Component",
    "Backlight Compensation", "Gain", "Power Line Frequency",
    "Hue Auto", "WB Temp Auto", "WB Component Auto",
    "Digital Multiplier", "Digital Multiplier Limit",
    "Analog Video Standard", "Analog Video Lock", "Contrast Auto"
]


def decode_bitmap(bitmap, names):
    """Decode control bitmap to list of control names."""
    controls = []
    for i, name in enumerate(names):
        if bitmap & (1 << i):
            controls.append(name)
    return controls


def identify_guid(guid):
    """Identify format from GUID."""
    if guid == GUID_YUY2:
        return "YUY2"
    elif guid == GUID_UYVY:
        return "UYVY"
    elif guid == GUID_NV12:
        return "NV12"
    else:
        return f"Unknown ({guid[:4].hex()})"


def analyze_device(dev):
    """Analyze USB device descriptors."""
    print("=" * 60)
    print("USB CAMERA ANALYSIS")
    print("=" * 60)
    print(f"Device: {dev.product}")
    print(f"Manufacturer: {dev.manufacturer}")
    print(f"VID:PID: {dev.idVendor:04x}:{dev.idProduct:04x}")
    print()

    # Get raw configuration descriptor
    raw_cfg = dev.ctrl_transfer(0x80, 6, 0x0200, 0, 512)

    formats = []
    frames = []
    camera_controls = 0
    processing_controls = 0
    uvc_version = None
    current_interface = None

    # Parse descriptors
    i = 0
    while i < len(raw_cfg):
        length = raw_cfg[i]
        if length == 0:
            break
        desc_type = raw_cfg[i + 1]
        desc = raw_cfg[i:i + length]

        if desc_type == 0x04:  # Interface descriptor
            intf_class = desc[5]
            intf_subclass = desc[6]
            current_interface = (intf_class, intf_subclass)

        elif desc_type == CS_INTERFACE and current_interface:
            subtype = desc[2]
            intf_class, intf_subclass = current_interface

            # Video Control interface
            if intf_class == 14 and intf_subclass == 1:
                if subtype == VC_HEADER and length >= 12:
                    uvc_version = (desc[4] << 8) | desc[3]
                elif subtype == VC_INPUT_TERMINAL and length >= 18:
                    ttype = (desc[5] << 8) | desc[4]
                    if ttype == 0x0201:  # Camera
                        camera_controls = desc[15] | (desc[16] << 8) | (desc[17] << 16)
                elif subtype == VC_PROCESSING_UNIT and length >= 10:
                    ctrl_size = desc[7]
                    for j in range(min(ctrl_size, 3)):
                        processing_controls |= desc[8 + j] << (8 * j)

            # Video Streaming interface
            elif intf_class == 14 and intf_subclass == 2:
                if subtype == VS_FORMAT_UNCOMPRESSED and length >= 21:
                    fmt_idx = desc[3]
                    guid = bytes(desc[5:21])
                    formats.append((fmt_idx, "Uncompressed", identify_guid(guid)))
                elif subtype == VS_FORMAT_MJPEG:
                    fmt_idx = desc[3]
                    formats.append((fmt_idx, "MJPEG", "MJPEG"))
                elif subtype in (VS_FRAME_UNCOMPRESSED, VS_FRAME_MJPEG) and length >= 9:
                    frame_idx = desc[3]
                    width = (desc[6] << 8) | desc[5]
                    height = (desc[8] << 8) | desc[7]
                    frames.append((frame_idx, width, height))

        i += length

    # Print results
    if uvc_version:
        print(f"UVC Version: {uvc_version >> 8}.{(uvc_version & 0xFF):02d}")
    print()

    print("FORMATS:")
    for idx, fmt_type, fmt_name in formats:
        print(f"  [{idx}] {fmt_type}: {fmt_name}")
    print()

    print("RESOLUTIONS:")
    for idx, w, h in frames:
        print(f"  [{idx}] {w}x{h}")
    print()

    print("CAMERA CONTROLS:")
    if camera_controls == 0:
        print("  None (fixed settings)")
    else:
        print(f"  Bitmap: 0x{camera_controls:06x}")
        for ctrl in decode_bitmap(camera_controls, CAMERA_CONTROLS):
            print(f"    - {ctrl}")
    print()

    print("PROCESSING CONTROLS:")
    if processing_controls == 0:
        print("  None (fixed settings)")
    else:
        print(f"  Bitmap: 0x{processing_controls:06x}")
        for ctrl in decode_bitmap(processing_controls, PROCESSING_CONTROLS):
            print(f"    - {ctrl}")
    print()

    print("=" * 60)


def main():
    if len(sys.argv) > 1:
        vid_pid = sys.argv[1]
        vid, pid = vid_pid.split(":")
        vid = int(vid, 16) if vid.startswith("0x") else int(vid)
        pid = int(pid, 16) if pid.startswith("0x") else int(pid)
        dev = usb.core.find(idVendor=vid, idProduct=pid)
    else:
        # Find first UVC device
        dev = usb.core.find(bDeviceClass=0xEF, bDeviceSubClass=0x02)

    if dev is None:
        print("No USB camera found")
        sys.exit(1)

    analyze_device(dev)


if __name__ == "__main__":
    main()
```

## ffmpeg/ffprobe Command Reference

### List Devices

```bash
# macOS
ffmpeg -f avfoundation -list_devices true -i "" 2>&1

# Linux
ffmpeg -f v4l2 -list_formats all -i /dev/video0 2>&1
```

### Probe Device

```bash
# macOS - specific pixel format
ffprobe -f avfoundation -pixel_format uyvy422 -framerate 30 \
  -video_size 640x480 -i "0" -show_streams 2>&1

# Linux
ffprobe -f v4l2 -video_size 640x480 -i /dev/video0 -show_streams 2>&1
```

### Debug Stream

```bash
# macOS - verbose debug output
ffplay -f avfoundation -pixel_format uyvy422 -framerate 30 \
  -video_size 640x480 -loglevel debug -i "0" 2>&1 | head -200

# Capture single frame
ffmpeg -f avfoundation -i "0" -frames:v 1 -f image2 frame.png
```

### Key ffprobe Fields

```bash
# Extract specific fields
ffprobe -f avfoundation -i "0" -show_entries \
  stream=codec_name,pix_fmt,width,height,r_frame_rate \
  -of csv=p=0 2>/dev/null
```

## uvc-util Command Reference

```bash
# List all UVC devices
uvc-util -d

# Select device and list controls
uvc-util -I 0 -c                    # By index
uvc-util -V 0x349c:0x0411 -c        # By VID:PID
uvc-util -N "HD camera" -c          # By name

# Show control details
uvc-util -I 0 -S brightness         # Single control
uvc-util -I 0 -S '*'                # All controls

# Get/set control values
uvc-util -I 0 -g brightness         # Get value
uvc-util -I 0 -o brightness         # Get value only (no label)
uvc-util -I 0 -s brightness=0.5     # Set to 50%
uvc-util -I 0 -s brightness=default # Reset to default
uvc-util -I 0 -r                    # Reset all controls

# Multi-component controls
uvc-util -I 0 -s pan-tilt-abs="{pan=-3600, tilt=0}"
```

## V4L2 Control Reference (Linux)

```bash
# List all controls
v4l2-ctl -d /dev/video0 --list-ctrls

# Get control value
v4l2-ctl -d /dev/video0 --get-ctrl=brightness

# Set control value
v4l2-ctl -d /dev/video0 --set-ctrl=brightness=128

# List formats and resolutions
v4l2-ctl -d /dev/video0 --list-formats-ext

# Get current format
v4l2-ctl -d /dev/video0 --get-fmt-video
```
