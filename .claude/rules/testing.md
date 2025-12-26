# Testing Requirements

## Rust Backend

Run checks before committing:
```bash
cargo check
cargo clippy
cargo test
```

## Frontend

Type-check Svelte/TypeScript:
```bash
npm run check
```

## Android Testing

Test on physical device (emulators lack USB OTG support).

### ADB over WiFi Setup

Since most phones have only one USB-C port (needed for the endoscope), use ADB over WiFi:

1. Connect phone via USB temporarily
2. Enable TCP/IP: `adb tcpip 5555`
3. Get phone IP: `adb shell ip addr show wlan0 | grep "inet "`
4. Connect wirelessly: `adb connect <PHONE_IP>:5555`
5. Unplug USB cable

See the main CLAUDE.md for detailed instructions and troubleshooting.

### Testing Workflow

1. Ensure ADB is connected over WiFi (see above)
2. Run `just android-dev` to deploy the app
3. Plug USB endoscope into phone's USB-C port
4. App should auto-launch (via intent filter)
5. Monitor logs: `just logs`

### What to Verify

- App launches when endoscope is plugged in
- USB permission is auto-granted (no dialog)
- Video feed displays in the UI
- Logs show frame reception: `adb logcat -s CleanScope:*`

## Coverage Expectations

- Tauri commands: Unit tests for business logic
- USB handling: Integration tests where possible, manual device testing required
- Frontend: Component tests for UI state transitions
