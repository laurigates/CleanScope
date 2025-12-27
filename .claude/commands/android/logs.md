Stream filtered CleanScope logs from Android device.

Arguments:
- $ARGUMENTS: --clear to clear logs first, --usb-only for USB events only

Steps:
1. If --clear: Run `adb logcat -c`
2. Stream with filter:
   - Default: `adb logcat -s CleanScope:* RustStdoutStderr:* AndroidRuntime:E`
   - --usb-only: `adb logcat -d | grep -E "(USB|UVC|libusb)" | tail -100`
3. Format output for readability

Output:
- Filtered log stream with timestamps
- Highlighted errors in red
- USB/UVC events highlighted if --usb-only
