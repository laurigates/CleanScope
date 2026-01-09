Quick USB camera probe to identify pixel format, resolution, and frame rate.

Arguments:
- $1: Device identifier (optional)
  - macOS: device index (0, 1, 2...) or name ("HD camera")
  - Linux: device path (/dev/video0)
  - Default: 0 (first video device)

Steps:

1. Detect platform:
   - macOS: Use AVFoundation via ffmpeg
   - Linux: Use V4L2 via v4l2-ctl/ffmpeg

2. List available devices:
   ```bash
   # macOS
   ffmpeg -f avfoundation -list_devices true -i "" 2>&1 | grep -A 10 "video devices"

   # Linux
   v4l2-ctl --list-devices
   ```

3. Probe target device:
   ```bash
   # macOS
   ffprobe -f avfoundation -framerate 30 -video_size 640x480 -i "$DEVICE" \
     -show_format -show_streams 2>&1

   # Linux
   ffprobe -f v4l2 -i "$DEVICE" -show_format -show_streams 2>&1
   ```

4. Extract key information:
   - `pix_fmt` - Pixel format (uyvy422, yuyv422, nv12, mjpeg)
   - `width` / `height` - Resolution
   - `r_frame_rate` - Frame rate
   - `codec_tag_string` - FourCC code

5. Check supported formats:
   ```bash
   # macOS - look for "Supported pixel formats" in stderr
   # Linux
   v4l2-ctl -d "$DEVICE" --list-formats-ext
   ```

Output:
- Device name and index
- Current pixel format with FourCC
- Resolution and frame rate
- List of supported formats
- Recommended CleanScope PixelFormat setting

Example output:
```
Device: HD camera (index 0)
VID:PID: 349c:0411

Current Configuration:
  Pixel Format: uyvy422 (UYVY)
  Resolution: 640x480
  Frame Rate: 17.17 fps
  Frame Size: 614400 bytes

Supported Formats:
  - uyvy422 (preferred)
  - yuyv422
  - nv12
  - 0rgb
  - bgr0

Recommendation:
  CleanScope should use PixelFormat::Uyvy for this device
```
