# CleanScope Development Commands and Skills

This document outlines proposed Claude Code commands, skills, and justfile recipes tailored for CleanScope development workflows.

## Proposed Commands

Commands are stored in `.claude/commands/` and invoked via `/command:name` syntax.

---

### `/android:build`

**Priority:** High

**Description:** Build Android APK with proper environment validation.

**What it does:**
1. Validates Android environment (ANDROID_HOME, NDK_HOME)
2. Runs `just android-build` to create debug APK
3. Reports APK location and size
4. Optionally runs `adb install` if device is connected

**Example usage:**
```
/android:build
/android:build --release
```

**Implementation file:** `.claude/commands/android/build.md`

```markdown
Build the CleanScope Android APK.

Arguments:
- $ARGUMENTS: Optional flags (--release for production build)

Steps:
1. Run `just check-prereqs` to validate environment
2. If --release: Run `just android-release`
3. Otherwise: Run `just android-build`
4. Report: APK path, size, and build time
5. If device connected: Offer to install with `adb install -r <apk>`
```

---

### `/android:deploy`

**Priority:** High

**Description:** Deploy app to connected Android device with log streaming setup.

**What it does:**
1. Checks for connected device (USB or WiFi)
2. Force-stops any running instance
3. Installs APK
4. Launches the app
5. Starts filtered log streaming

**Example usage:**
```
/android:deploy
/android:deploy --wifi 192.168.1.42
```

**Implementation file:** `.claude/commands/android/deploy.md`

```markdown
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
```

---

### `/android:logs`

**Priority:** High

**Description:** Show filtered app logs with USB/UVC debugging focus.

**What it does:**
1. Clears previous logs (optional)
2. Streams logs filtered by CleanScope, USB, and UVC patterns
3. Highlights frame reception and error messages

**Example usage:**
```
/android:logs
/android:logs --clear
/android:logs --usb-only
```

**Implementation file:** `.claude/commands/android/logs.md`

```markdown
Stream filtered CleanScope logs from Android device.

Arguments:
- $ARGUMENTS: --clear to clear logs first, --usb-only for USB events only

Steps:
1. If --clear: Run `adb logcat -c`
2. Stream with filter:
   - Default: `adb logcat -s CleanScope:* RustStdoutStderr:* AndroidRuntime:E`
   - --usb-only: `adb logcat -d | grep -E "(USB|UVC|libusb)" | tail -100`
3. Format output for readability
```

---

### `/android:wifi`

**Priority:** Medium

**Description:** Set up ADB over WiFi for USB endoscope testing.

**What it does:**
1. Enables ADB TCP/IP mode on connected device
2. Gets device IP address
3. Connects wirelessly
4. Verifies connection
5. Instructions for unplugging USB

**Example usage:**
```
/android:wifi
```

**Implementation file:** `.claude/commands/android/wifi.md`

```markdown
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
```

---

### `/usb:test`

**Priority:** High

**Description:** USB endoscope testing workflow with diagnostics.

**What it does:**
1. Verifies ADB connection (preferably WiFi)
2. Deploys latest app version
3. Monitors for USB_DEVICE_ATTACHED intent
4. Streams UVC-specific logs
5. Reports frame reception statistics

**Example usage:**
```
/usb:test
/usb:test --diagnose
```

**Implementation file:** `.claude/commands/usb/test.md`

```markdown
USB endoscope testing workflow.

Arguments:
- $ARGUMENTS: --diagnose for extended diagnostics

Steps:
1. Check ADB connection type (warn if USB, not WiFi)
2. Deploy app: `/android:deploy`
3. Instruct: "Plug USB endoscope into phone"
4. Monitor for intent: `adb logcat -d | grep USB_DEVICE_ATTACHED`
5. Check permission: Look for "USB Permission Granted" in logs
6. Monitor frames: Look for "MJPEG frame" messages
7. Report: Frame count, sizes, any errors

If --diagnose:
- Dump USB device list: `adb shell dumpsys usb`
- Check intent filters: Review AndroidManifest.xml
- Verify device_filter.xml matches endoscope VID/PID
```

---

### `/adr:create`

**Priority:** Medium

**Description:** Create a new Architecture Decision Record.

**What it does:**
1. Prompts for ADR title
2. Determines next ADR number
3. Creates ADR file from template
4. Opens for editing

**Example usage:**
```
/adr:create Error Handling Strategy
/adr:create Reconnection Protocol
```

**Implementation file:** `.claude/commands/adr/create.md`

```markdown
Create a new Architecture Decision Record.

Arguments:
- $ARGUMENTS: ADR title (required)

Template location: .claude/blueprints/adr/

Steps:
1. Count existing ADRs in .claude/blueprints/adr/
2. Generate next number: 004, 005, etc.
3. Create filename: NNN-<kebab-case-title>.md
4. Use ADR template with sections:
   - Status (Proposed)
   - Context
   - Decision
   - Options Considered
   - Consequences
   - Related Decisions
   - References
5. Update .manifest.json if present
```

---

### `/tauri:command`

**Priority:** Medium

**Description:** Add a new Tauri command with proper boilerplate.

**What it does:**
1. Prompts for command name and parameters
2. Adds function to lib.rs with `#[tauri::command]`
3. Registers in `invoke_handler`
4. Generates TypeScript invoke call example

**Example usage:**
```
/tauri:command get_usb_devices
/tauri:command set_resolution width:u32 height:u32
```

**Implementation file:** `.claude/commands/tauri/command.md`

```markdown
Add a new Tauri command.

Arguments:
- $ARGUMENTS: command_name [param:type ...]

Steps:
1. Parse command name and parameters
2. Add to src-tauri/src/lib.rs:
   - #[tauri::command] function
   - Return type Result<T, String>
3. Register in invoke_handler macro
4. Generate frontend example:
   ```typescript
   const result = await invoke("command_name", { param: value });
   ```
5. Run `cargo check` to verify
```

---

### `/check:android`

**Priority:** Medium

**Description:** Run full Android build verification.

**What it does:**
1. Runs all Rust checks (cargo check, clippy)
2. Runs frontend checks (npm run check)
3. Attempts Android build
4. Reports any issues

**Example usage:**
```
/check:android
```

**Implementation file:** `.claude/commands/check/android.md`

```markdown
Run complete Android build verification.

Steps:
1. Run `just check-prereqs`
2. Run `just rust-check` (cargo check)
3. Run `just rust-clippy` (cargo clippy)
4. Run `just typecheck` (npm run check)
5. Run `just android-build`
6. Report: Build success/failure, warnings, APK size
```

---

## Proposed Skills

Skills are stored in `.claude/skills/` and provide reusable knowledge/patterns.

---

### `tauri-android`

**Priority:** High

**Description:** Tauri v2 Android development patterns and troubleshooting.

**Location:** `.claude/skills/tauri-android/SKILL.md`

**Contents:**
- Android environment setup (ANDROID_HOME, NDK_HOME)
- Rust cross-compilation targets (aarch64-linux-android)
- Tauri Android build process
- Common build errors and solutions
- Platform-specific code patterns (`#[cfg(target_os = "android")]`)
- android_logger initialization
- JNI basics for Android API access

**Reference file:** `.claude/skills/tauri-android/REFERENCE.md`
- Build configuration in tauri.conf.json
- Android manifest customization
- Intent filters for USB devices
- Gradle configuration locations

---

### `uvc-streaming`

**Priority:** High

**Description:** UVC camera streaming implementation patterns.

**Location:** `.claude/skills/uvc-streaming/SKILL.md`

**Contents:**
- UVC protocol overview (Probe/Commit negotiation)
- Control transfer structure (request types, control selectors)
- UvcStreamControl structure (26-34 bytes)
- Bulk transfer for MJPEG frames
- Frame boundary detection (header flags)
- MJPEG frame validation (SOI/EOI markers)
- Common UVC error codes and meanings

**Reference file:** `.claude/skills/uvc-streaming/REFERENCE.md`
- USB Video Class 1.1/1.5 constants
- Endpoint addressing conventions
- Frame descriptor parsing (future)
- libusb error handling

---

### `jni-android`

**Priority:** Medium

**Description:** JNI patterns for Android API access from Rust.

**Location:** `.claude/skills/jni-android/SKILL.md`

**Contents:**
- ndk-context usage for getting VM and Activity
- JavaVM and JNIEnv lifecycle
- JNI method calls (static, instance)
- Type signatures for common Android types
- UsbManager access pattern
- Permission checking via JNI
- Error handling and null checks

**Reference file:** `.claude/skills/jni-android/REFERENCE.md`
- Common JNI type mappings
- Android USB API JNI signatures
- Thread attachment patterns
- Local/global reference management

---

### `libusb-android`

**Priority:** Medium

**Description:** libusb integration on Android via file descriptor wrapping.

**Location:** `.claude/skills/libusb-android/SKILL.md`

**Contents:**
- Why direct device enumeration fails on Android (SELinux)
- libusb_wrap_sys_device() pattern
- libusb1-sys with vendored feature
- Context creation for Android (no discovery)
- Interface claiming
- Bulk and control transfers
- Error code interpretation

---

### `frame-streaming`

**Priority:** Medium

**Description:** Tauri IPC patterns for binary data streaming.

**Location:** `.claude/skills/frame-streaming/SKILL.md`

**Contents:**
- ipc::Response for binary data (no Base64)
- Polling pattern vs event streaming
- Arc<Mutex<FrameBuffer>> shared state
- Event emission for notifications
- Frontend ArrayBuffer handling
- createImageBitmap() for JPEG decode
- Canvas rendering patterns

---

## Proposed Justfile Recipes

These recipes would enhance the existing justfile.

---

### `adb-wifi` (High Priority)

```just
# Set up ADB over WiFi for USB endoscope testing
adb-wifi ip="":
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "{{ip}}" ]]; then
        echo "Enabling ADB TCP/IP mode..."
        adb tcpip 5555
        echo ""
        echo "Get your phone's IP address:"
        adb shell ip addr show wlan0 | grep "inet "
        echo ""
        echo "Then run: just adb-wifi <IP>"
    else
        echo "Connecting to {{ip}}:5555..."
        adb connect "{{ip}}:5555"
        echo ""
        echo "Verifying connection..."
        adb devices
        echo ""
        echo "You can now unplug the USB cable."
        echo "The endoscope can be plugged into the phone's USB-C port."
    fi
```

---

### `usb-debug` (High Priority)

```just
# Show USB-related logs and device info
usb-debug:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Connected USB Devices ==="
    adb shell dumpsys usb | grep -A5 "USB Device" || echo "No USB devices"
    echo ""
    echo "=== Recent USB Logs ==="
    adb logcat -d | grep -E "(USB|UVC|libusb|CleanScope)" | tail -50
```

---

### `force-stop` (Medium Priority)

```just
# Force stop the app
force-stop:
    adb shell am force-stop com.cleanscope.app
    @echo "App stopped"
```

---

### `launch` (Medium Priority)

```just
# Launch the app without rebuilding
launch:
    adb shell am start -n com.cleanscope.app/.MainActivity
    @echo "App launched"
```

---

### `reinstall` (Medium Priority)

```just
# Uninstall and reinstall (clears app data)
reinstall: android-uninstall android-install launch
    @echo "App reinstalled with fresh data"
```

---

### `adr-list` (Low Priority)

```just
# List all Architecture Decision Records
adr-list:
    @echo "Architecture Decision Records:"
    @ls -1 .claude/blueprints/adr/*.md 2>/dev/null | grep -v manifest | while read f; do \
        title=$(head -1 "$f" | sed 's/^# //'); \
        echo "  $(basename "$f"): $title"; \
    done
```

---

### `frame-stats` (Low Priority)

```just
# Show frame streaming statistics from logs
frame-stats:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Frame Streaming Statistics ==="
    LOGS=$(adb logcat -d | grep -E "MJPEG frame|Received.*frames")
    echo "$LOGS" | tail -20
    echo ""
    FRAME_COUNT=$(echo "$LOGS" | grep -oE "Received [0-9]+ frames" | tail -1)
    echo "Latest: $FRAME_COUNT"
```

---

## Summary by Priority

### High Priority (Implement First)

| Type | Name | Purpose |
|------|------|---------|
| Command | `/android:build` | Build APK |
| Command | `/android:deploy` | Deploy to device |
| Command | `/android:logs` | Filtered log streaming |
| Command | `/usb:test` | USB testing workflow |
| Skill | `tauri-android` | Tauri Android patterns |
| Skill | `uvc-streaming` | UVC protocol knowledge |
| Recipe | `adb-wifi` | ADB over WiFi setup |
| Recipe | `usb-debug` | USB diagnostics |

### Medium Priority

| Type | Name | Purpose |
|------|------|---------|
| Command | `/android:wifi` | WiFi ADB setup |
| Command | `/adr:create` | Create new ADR |
| Command | `/tauri:command` | Add Tauri command |
| Command | `/check:android` | Full build check |
| Skill | `jni-android` | JNI patterns |
| Skill | `libusb-android` | libusb on Android |
| Skill | `frame-streaming` | IPC binary streaming |
| Recipe | `force-stop` | Stop app |
| Recipe | `launch` | Launch app |
| Recipe | `reinstall` | Clean reinstall |

### Low Priority

| Type | Name | Purpose |
|------|------|---------|
| Recipe | `adr-list` | List ADRs |
| Recipe | `frame-stats` | Frame statistics |

---

## Implementation Notes

### Command Implementation Pattern

Commands in `.claude/commands/<group>/<name>.md` follow this structure:

```markdown
<Description of what the command does>

Arguments:
- $ARGUMENTS: Description of expected arguments

Steps:
1. First action
2. Second action
3. ...

Output:
<Description of expected output format>
```

### Skill Implementation Pattern

Skills in `.claude/skills/<name>/SKILL.md` follow this structure:

```markdown
# <Skill Name>

## Overview
Brief description of the skill domain.

## When to Use
- Situation 1
- Situation 2

## Key Concepts
### Concept 1
Explanation...

### Concept 2
Explanation...

## Common Patterns
### Pattern 1
```code
Example...
```

## Troubleshooting
| Issue | Solution |
|-------|----------|
| ... | ... |

## References
- Link 1
- Link 2
```

### Integration with Existing Infrastructure

- Commands should use `just` recipes where possible
- Skills should reference ADRs for architectural decisions
- New recipes should follow existing justfile conventions
- All Android commands should validate environment first
