Discover UVC controls available on a USB camera.

Arguments:
- $1: Device identifier
  - Device index (0, 1, 2...)
  - VID:PID format (e.g., "349c:0411")
  - Device name (e.g., "HD camera")
  - Default: 0

Steps:

1. Detect platform and available tools:
   - macOS: Check for uvc-util
   - Linux: Use v4l2-ctl
   - Fallback: pyusb descriptor parsing

2. Using uvc-util (macOS):
   ```bash
   # List devices
   uvc-util -d

   # List controls for device
   uvc-util -I $INDEX -c

   # Show all control details
   uvc-util -I $INDEX -S '*'
   ```

3. Using v4l2-ctl (Linux):
   ```bash
   # List all controls with current values
   v4l2-ctl -d /dev/video$INDEX --list-ctrls

   # Detailed control info
   v4l2-ctl -d /dev/video$INDEX --list-ctrls-menus
   ```

4. Using pyusb (fallback - all platforms):
   ```python
   # Parse camera terminal control bitmap (bytes 15-17 of INPUT_TERMINAL)
   # Parse processing unit control bitmap (bytes 8+ of PROCESSING_UNIT)
   # See REFERENCE.md for full script
   ```

5. Interpret control bitmaps:
   - Camera Terminal Controls: scanning, exposure, focus, zoom, pan/tilt
   - Processing Unit Controls: brightness, contrast, hue, saturation, gain

6. For each available control, report:
   - Control name
   - Current value
   - Min/max/default values (if available)
   - Step size

Output:
```
UVC CONTROL DISCOVERY
=====================
Device: HD camera (349c:0411)

CAMERA TERMINAL CONTROLS
------------------------
Control Bitmap: 0x000000

No camera controls available.
This device has fixed camera settings (no exposure, focus, zoom control).

PROCESSING UNIT CONTROLS
------------------------
Control Bitmap: 0x000000

No processing controls available.
This device has fixed image processing (no brightness, contrast, gain control).

SUMMARY
-------
This is a fixed-function camera with NO adjustable controls.

Common for cheap USB endoscopes. Brightness, contrast, and other
adjustments must be done in software post-processing.

CleanScope Recommendation:
- Consider adding software brightness/contrast sliders
- No hardware control UI needed for this device
```

Alternative output (for cameras with controls):
```
UVC CONTROL DISCOVERY
=====================
Device: Logitech HD Webcam (046d:0825)

CAMERA TERMINAL CONTROLS
------------------------
Control Bitmap: 0x03002f
Available:
  - Scanning Mode
  - Auto-Exposure Mode
  - Auto-Exposure Priority
  - Exposure Time (Absolute)
  - Focus (Absolute)
  - Focus Auto

PROCESSING UNIT CONTROLS
------------------------
Control Bitmap: 0x177f
Available:
  - Brightness        [0-255, default=128]
  - Contrast          [0-255, default=32]
  - Hue               [0-255, default=128]
  - Saturation        [0-255, default=32]
  - Sharpness         [0-255, default=24]
  - Gamma             [100-500, default=300]
  - White Balance Temp [2800-6500, default=4600]
  - Backlight Compensation [0-2, default=1]
  - Gain              [0-255, default=0]
  - Power Line Frequency [0=Off, 1=50Hz, 2=60Hz]
  - White Balance Temp Auto [0-1, default=1]
  - Sharpness         [0-255, default=24]
```
