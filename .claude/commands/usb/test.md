USB endoscope testing workflow on Android device.

Arguments:
- $ARGUMENTS: --diagnose for extended diagnostics, --analyze for camera analysis

Steps:
1. Check ADB connection type (warn if USB, not WiFi)
2. Deploy app: Run `/android:deploy` workflow
3. Instruct: "Plug USB endoscope into phone"
4. Monitor for intent: `adb logcat -d | grep USB_DEVICE_ATTACHED`
5. Check permission: Look for "USB Permission Granted" in logs
6. Monitor frames: Look for "YUY2 frame\|MJPEG frame" messages
7. Report: Frame count, sizes, any errors

If --diagnose:
- Dump USB device list: `adb shell dumpsys usb`
- Check intent filters: Review AndroidManifest.xml
- Verify device_filter.xml matches endoscope VID/PID

If --analyze (run on host machine before Android testing):
- Run `/usb:analyze` to probe camera on macOS/Linux
- Identify pixel format, controls, and recommendations
- Ensure device_filter.xml has correct VID/PID

Output:
- Connection type warning
- USB device detection status
- Permission grant status
- Frame reception statistics
- Extended diagnostics if requested

See also:
- `/usb:probe` - Quick camera probe on host machine
- `/usb:descriptors` - Detailed USB descriptor analysis
- `/usb:controls` - UVC control discovery
- `/usb:analyze` - Comprehensive analysis with recommendations
