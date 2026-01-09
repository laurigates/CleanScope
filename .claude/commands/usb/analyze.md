Comprehensive USB camera analysis combining probe, descriptors, and controls.

Arguments:
- $1: Device identifier (optional)
  - macOS: device index (0) or name ("HD camera")
  - Linux: device path (/dev/video0)
  - VID:PID (349c:0411)
  - Default: Auto-detect first video device

This command runs all analysis steps and generates a comprehensive report with CleanScope configuration recommendations.

Steps:

1. Device Discovery:
   - List all available video devices
   - Identify target device
   - Get VID/PID and basic USB info

2. Run `/usb:probe`:
   - Detect current pixel format
   - Get resolution and frame rate
   - List supported formats

3. Run `/usb:descriptors`:
   - Parse UVC descriptors
   - List all format/frame combinations
   - Identify UVC version

4. Run `/usb:controls`:
   - Check camera terminal controls
   - Check processing unit controls
   - Report available adjustments

5. Generate CleanScope recommendations:
   - device_filter.xml entry
   - Default PixelFormat setting
   - Software adjustments needed
   - Known quirks for this device type

Output:
```
================================================================================
                        USB CAMERA ANALYSIS REPORT
================================================================================

DEVICE INFORMATION
------------------
Name:           HD camera
Manufacturer:   Generic
VID:PID:        349c:0411
USB Speed:      High Speed (USB 2.0)
UVC Version:    1.00

VIDEO STREAM CONFIGURATION
--------------------------
Current Format: UYVY (uyvy422)
Resolution:     640x480
Frame Rate:     17.17 fps
Frame Size:     614,400 bytes
Stride:         1,280 bytes

SUPPORTED FORMATS
-----------------
Format 1: Uncompressed YUY2
  - 640x480
  - 320x240
  - 640x360
  - 480x360
  - 352x288
  - 176x144

Supported pixel formats (macOS AVFoundation):
  - uyvy422 (preferred)
  - yuyv422
  - nv12
  - 0rgb
  - bgr0

UVC CONTROLS
------------
Camera Controls:    NONE (0x000000)
Processing Controls: NONE (0x000000)

This device has NO adjustable controls.
All image parameters are fixed in hardware.

================================================================================
                        CLEANSCOPE RECOMMENDATIONS
================================================================================

1. DEVICE FILTER (src-tauri/gen/android/.../device_filter.xml)
   Add this entry for auto-permission on Android:

   <usb-device vendor-id="13468" product-id="1041" />

2. DEFAULT PIXEL FORMAT
   Camera prefers UYVY format. Set default in StreamingConfig:

   pixel_format: PixelFormat::Uyvy

3. SOFTWARE ADJUSTMENTS NEEDED
   Since hardware controls are unavailable, consider adding:
   - Brightness slider (add offset to Y channel)
   - Contrast slider (multiply Y channel around midpoint)
   - Gamma correction curve

4. KNOWN CHARACTERISTICS
   - Cheap endoscope with fixed settings
   - Only YUY2 uncompressed format (no MJPEG)
   - ~17 fps at 640x480
   - May report as YUYV but macOS converts to UYVY

5. POTENTIAL ISSUES
   - No auto-exposure: may need LED lighting
   - No auto-focus: fixed focus distance
   - No white balance: color temperature fixed

================================================================================
```

Error Handling:
- If device not found: List available devices and prompt for selection
- If pyusb not installed: Provide installation instructions
- If uvc-util not available: Fall back to descriptor parsing
- If ffprobe fails: Check permissions and device availability
