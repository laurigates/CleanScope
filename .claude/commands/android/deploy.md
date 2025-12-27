Deploy CleanScope to connected Android device.

Arguments:
- $ARGUMENTS: Optional --wifi <IP> for ADB over WiFi setup

Steps:
1. Check `adb devices` for connected device
2. If --wifi provided: Run `adb connect <IP>:5555`
3. Run `adb shell am force-stop com.cleanscope.app`
4. Run `just android-install` or `adb install -r <apk>`
5. Launch: `adb shell am start -n com.cleanscope.app/.MainActivity`
6. Stream logs: `adb logcat -s CleanScope:* RustStdoutStderr:* AndroidRuntime:E`

Output:
- Device connection status
- Installation result
- App launch confirmation
- Live log stream
