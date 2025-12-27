Set up ADB over WiFi for USB endoscope testing.

This is essential because the phone's USB-C port is needed for the endoscope.

Steps:
1. Verify device connected via USB: `adb devices`
2. Enable TCP/IP: `adb tcpip 5555`
3. Get IP: `adb shell ip addr show wlan0 | grep "inet "`
4. Connect: `adb connect <IP>:5555`
5. Verify: `adb devices` should show both connections
6. Instruct user to unplug USB cable
7. Verify WiFi-only: `adb devices` should show only wireless

Output:
- Step-by-step progress with status
- Phone's IP address
- Connection verification
- Clear instructions for unplugging USB
