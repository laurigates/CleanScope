USB endoscope testing workflow.

Arguments:
- $ARGUMENTS: --diagnose for extended diagnostics

Steps:
1. Check ADB connection type (warn if USB, not WiFi)
2. Deploy app: Run `/android:deploy` workflow
3. Instruct: "Plug USB endoscope into phone"
4. Monitor for intent: `adb logcat -d | grep USB_DEVICE_ATTACHED`
5. Check permission: Look for "USB Permission Granted" in logs
6. Monitor frames: Look for "MJPEG frame" messages
7. Report: Frame count, sizes, any errors

If --diagnose:
- Dump USB device list: `adb shell dumpsys usb`
- Check intent filters: Review AndroidManifest.xml
- Verify device_filter.xml matches endoscope VID/PID

Output:
- Connection type warning
- USB device detection status
- Permission grant status
- Frame reception statistics
- Extended diagnostics if requested
