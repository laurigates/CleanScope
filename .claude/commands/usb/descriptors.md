Parse USB descriptors to discover UVC formats, resolutions, and device capabilities.

Arguments:
- $1: Device identifier
  - VID:PID format (e.g., "349c:0411" or "0x349c:0x0411")
  - Device name (e.g., "HD camera")
  - Default: Auto-detect first UVC device

Prerequisites:
- pyusb installed: `pip install pyusb`
- libusb backend available

Steps:

1. Find device:
   ```bash
   # By VID:PID
   python3 -c "import usb.core; d=usb.core.find(idVendor=0x$VID, idProduct=0x$PID); print(d)"

   # By name (search all devices)
   python3 -c "
   import usb.core
   for d in usb.core.find(find_all=True):
       if d.product and '$NAME' in d.product:
           print(f'{d.idVendor:04x}:{d.idProduct:04x} - {d.product}')
   "
   ```

2. Get basic device info:
   ```bash
   # macOS
   ioreg -r -c IOUSBHostDevice -n "$NAME" -l | grep -E "idVendor|idProduct|USB Product|bDeviceClass"
   ```

3. Parse UVC descriptors using the pyusb script from REFERENCE.md:
   - Read raw configuration descriptor
   - Parse VC_HEADER for UVC version
   - Parse INPUT_TERMINAL for camera type
   - Parse PROCESSING_UNIT for processing controls
   - Parse VS_FORMAT_* for available formats
   - Parse VS_FRAME_* for available resolutions

4. Report findings:
   - UVC version (1.00, 1.10, 1.50)
   - Available formats with GUIDs (YUY2, UYVY, NV12, MJPEG)
   - Available resolutions for each format
   - Control unit configurations

Output:
```
USB DEVICE DESCRIPTORS
======================
Device: HD camera
VID:PID: 349c:0411
UVC Version: 1.00

VIDEO CONTROL INTERFACE
-----------------------
Input Terminal ID=1: Camera (0x0201)
  Camera Controls: 0x000000 (none)

Processing Unit ID=2:
  Source: 1
  Processing Controls: 0x000000 (none)

Output Terminal ID=3: USB Streaming (0x0101)
  Source: 2

VIDEO STREAMING INTERFACE
-------------------------
Format 1: Uncompressed (YUY2)
  Frame 1: 640x480
  Frame 2: 320x240
  Frame 3: 640x360
  Frame 4: 480x360
  Frame 5: 352x288
  Frame 6: 176x144

SUMMARY
-------
- Single format: YUY2 (uncompressed)
- 6 resolutions available
- No UVC controls (fixed settings camera)
- Recommended: Add VID:PID to device_filter.xml
```
