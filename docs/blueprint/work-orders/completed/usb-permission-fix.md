# Work Order: USB Permission Fix

## Status: Completed ✅
**Created:** 2025-12-26
**Completed:** 2026-01-17
**Priority:** High

### Resolution
The fix was implemented using **Option 2** (get device from intent) combined with **Option 1** (no hasPermission check):
- `get_device_name_from_intent()` extracts device name from launching intent's `EXTRA_DEVICE`
- `get_device_from_manager()` looks up device by name in `getDeviceList()` with fallback
- `get_usb_file_descriptor()` orchestrates the flow without `hasPermission()` check

Code location: `src-tauri/src/usb.rs` lines 163-359

---

## Problem Statement

The CleanScope app correctly launches when a USB endoscope is plugged in (Android intent filter works), but immediately shows "Disconnected" status because the USB permission check fails.

### Observed Behavior

1. User plugs in USB endoscope
2. Android system shows permission dialog: "Allow CleanScope to access HD camera?"
3. User selects "Always" to grant persistent permission
4. App launches via `USB_DEVICE_ATTACHED` intent
5. App shows "Disconnected" instead of video feed

### Log Evidence (19:49:56)

```
CleanScope: clean_scope_lib::usb: Initializing USB handler
CleanScope: clean_scope_lib::usb: No USB permission for device
CleanScope: clean_scope_lib::usb: No USB device found on startup
```

System logs show permission was granted:
```
grantDevicePermission: device=/dev/bus/usb/001/002 For uid=10027
```

---

## Root Cause Analysis

### How Android USB Permissions Work

When an app is launched via `USB_DEVICE_ATTACHED` intent:

1. **Intent carries the UsbDevice**: The triggering `UsbDevice` object is included in intent extras
2. **Implicit permission**: The device from the intent has implicit permission (no need to call `requestPermission()`)
3. **Timing matters**: Permission is valid for the device attached to the intent, not necessarily for devices queried via `UsbManager.getDeviceList()`

### Current Code Flow (Flawed)

File: `/Users/lgates/repos/laurigates/CleanScope/src-tauri/src/usb.rs`

```rust
// Lines 55-171: get_usb_file_descriptor()
fn get_usb_file_descriptor() -> Option<i32> {
    // 1. Gets UsbManager service
    // 2. Calls getDeviceList() to enumerate devices
    // 3. Iterates to find first device
    // 4. Calls hasPermission(device) <- FAILS HERE
    // 5. If permission granted, calls openDevice(device)
}
```

### Why It Fails

The code queries `UsbManager.getDeviceList()` and then checks `hasPermission()` on the first device found. This approach has two problems:

1. **Wrong device source**: The device from `getDeviceList()` may not be the same object as the one from the intent
2. **Permission race**: The `hasPermission()` check may execute before Android has fully registered the permission grant
3. **Unnecessary check**: When launched via USB intent, the intent's device already has implicit permission

### The Correct Flow

When app is launched via `USB_DEVICE_ATTACHED`:
1. Get `UsbDevice` from `activity.getIntent().getParcelableExtra(UsbManager.EXTRA_DEVICE)`
2. This device has implicit permission - no `hasPermission()` check needed
3. Call `openDevice(device)` directly

---

## Proposed Solutions

### Option 1: Skip Permission Check, Try Direct Open (Simplest)

**Risk:** Low
**Effort:** Minimal (5 lines changed)
**Recommendation:** Try this first

Remove the `hasPermission()` check entirely. Just try to open the device - if permission wasn't granted, `openDevice()` returns null, which we already handle.

**Changes to `/Users/lgates/repos/laurigates/CleanScope/src-tauri/src/usb.rs`:**

```rust
// Lines 126-141: REMOVE this block entirely
let has_permission = env
    .call_method(
        &usb_manager,
        "hasPermission",
        "(Landroid/hardware/usb/UsbDevice;)Z",
        &[JValue::Object(&device)],
    )
    .ok()?
    .z()
    .ok()?;

if !has_permission {
    log::warn!("No USB permission for device");
    return None;
}

log::info!("USB Permission Granted");
```

Replace with:
```rust
log::info!("Attempting to open USB device");
```

The existing null check on `connection` (lines 157-160) will handle the permission-denied case.

---

### Option 2: Get Device from Intent Extras (Most Correct)

**Risk:** Low
**Effort:** Moderate (20-30 lines)
**Recommendation:** Proper solution after Option 1 is validated

When launched via USB intent, extract the device directly from the intent extras where it has implicit permission.

**Changes to `/Users/lgates/repos/laurigates/CleanScope/src-tauri/src/usb.rs`:**

Add new function before `get_usb_file_descriptor()`:

```rust
/// Get USB device from the intent that launched this activity
/// This device has implicit permission when launched via USB_DEVICE_ATTACHED
fn get_device_from_intent(env: &mut JNIEnv, activity: &JObject) -> Option<JObject> {
    // Get the launching intent
    let intent = env
        .call_method(activity, "getIntent", "()Landroid/content/Intent;", &[])
        .ok()?
        .l()
        .ok()?;

    // Get UsbManager.EXTRA_DEVICE constant
    let extra_device_key = env
        .get_static_field(
            "android/hardware/usb/UsbManager",
            "EXTRA_DEVICE",
            "Ljava/lang/String;",
        )
        .ok()?
        .l()
        .ok()?;

    // Get the UsbDevice from intent extras
    let device = env
        .call_method(
            &intent,
            "getParcelableExtra",
            "(Ljava/lang/String;)Landroid/os/Parcelable;",
            &[JValue::Object(&extra_device_key)],
        )
        .ok()?
        .l()
        .ok()?;

    if device.is_null() {
        log::info!("No USB device in intent extras");
        return None;
    }

    log::info!("Got USB device from intent extras (has implicit permission)");
    Some(device)
}
```

Modify `get_usb_file_descriptor()` to try intent device first:

```rust
fn get_usb_file_descriptor() -> Option<i32> {
    // ... existing setup code ...

    // Try to get device from intent first (has implicit permission)
    let device = get_device_from_intent(&mut env, &activity)
        .or_else(|| {
            // Fallback: query device list (may need explicit permission)
            log::info!("Falling back to device list enumeration");
            get_first_device_from_manager(&mut env, &usb_manager)
        })?;

    // Skip hasPermission check - either we have implicit permission
    // from intent, or openDevice will fail and we handle it

    // ... existing openDevice code ...
}
```

---

### Option 3: Request Permission Explicitly (Most Robust)

**Risk:** Medium (requires Java-side changes)
**Effort:** High (50+ lines, Kotlin/Java callback handling)
**Recommendation:** Only if Options 1 & 2 fail

If `hasPermission()` returns false, call `requestPermission()` with a `PendingIntent`. This shows a permission dialog and requires handling the async callback.

**Not recommended** as first approach because:
- Requires modifying Java/Kotlin code in MainActivity
- Adds complexity with PendingIntent and BroadcastReceiver
- Permission should already be granted via intent filter + "Always" selection

---

## Implementation Guide

### Step 1: Apply Option 1 (Quick Fix)

1. Open `/Users/lgates/repos/laurigates/CleanScope/src-tauri/src/usb.rs`

2. Navigate to `get_usb_file_descriptor()` function (line 55)

3. Find the permission check block (lines 126-143):
   ```rust
   // Check if we have permission
   let has_permission = env
       .call_method(...)
   ```

4. Replace lines 126-143 with:
   ```rust
   // Note: When launched via USB_DEVICE_ATTACHED intent, the device
   // has implicit permission. When launched normally, openDevice()
   // will fail if we don't have permission, which we handle below.
   log::info!("Attempting to open USB device (skipping hasPermission check)");
   ```

5. Build and test:
   ```bash
   just android-build
   adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
   ```

6. Test procedure:
   ```bash
   adb shell am force-stop com.cleanscope.app
   # Unplug and re-plug USB endoscope
   adb logcat -d | grep -E "(CleanScope|USB)" | tail -50
   ```

7. Expected success logs:
   ```
   CleanScope: Initializing USB handler
   CleanScope: Attempting to open USB device (skipping hasPermission check)
   CleanScope: USB device found with fd: <number>
   CleanScope: Starting camera loop with fd: <number>
   ```

### Step 2: If Option 1 Fails, Apply Option 2

If logs show `Failed to open USB device` after Option 1:

1. Implement `get_device_from_intent()` function as shown above

2. Modify `get_usb_file_descriptor()` to try intent device first

3. Add detailed logging to identify which path is taken:
   ```rust
   log::info!("Attempting USB init: intent={}, manager={}",
       has_intent_device, has_manager_device);
   ```

### Step 3: Verification

After either fix, verify the complete flow:

```bash
# Terminal 1: Watch logs
adb logcat -s CleanScope:*

# Terminal 2: Test cycle
adb shell am force-stop com.cleanscope.app
# Physically unplug USB endoscope
# Wait 2 seconds
# Physically plug in USB endoscope
```

Expected log sequence:
1. `Initializing USB handler`
2. `Attempting to open USB device...`
3. `USB device found with fd: N`
4. `Starting camera loop with fd: N`
5. `libusb context created`
6. `Android FD wrapped successfully`
7. `Device: VID=xxxx PID=xxxx Class=xx`
8. `UVC streaming started on endpoint 0x81`
9. `MJPEG frame 1 received: NNNN bytes`

---

## Files to Modify

| File | Changes |
|------|---------|
| `src-tauri/src/usb.rs` | Remove `hasPermission()` check or add intent device extraction |

## Files for Reference (Do Not Modify)

| File | Purpose |
|------|---------|
| `src-tauri/gen/android/app/src/main/AndroidManifest.xml` | USB intent filter configuration |
| `src-tauri/gen/android/app/src/main/res/xml/device_filter.xml` | UVC class filters |
| `src-tauri/src/lib.rs` | App setup, calls `usb::init_usb_handler()` |

---

## Related Documentation

- [ADR-001: Frame Streaming Architecture](../adr/001-frame-streaming-architecture.md)
- [ADR-002: JPEG Decoding Strategy](../adr/002-jpeg-decoding-strategy.md)
- [ADR-003: Resolution Negotiation Protocol](../adr/003-resolution-negotiation-protocol.md)

## Android USB Documentation

- [USB Host Overview](https://developer.android.com/develop/connectivity/usb/host)
- [USB_DEVICE_ATTACHED Intent](https://developer.android.com/reference/android/hardware/usb/UsbManager#ACTION_USB_DEVICE_ATTACHED)
- [UsbManager.EXTRA_DEVICE](https://developer.android.com/reference/android/hardware/usb/UsbManager#EXTRA_DEVICE)

---

## Acceptance Criteria

- [ ] App shows "Connected" status when USB endoscope is plugged in
- [ ] Video feed displays in the UI
- [ ] Logs show successful fd acquisition and frame streaming
- [ ] No permission dialogs appear (permission via intent filter)
- [ ] App survives unplug/replug cycle

---

## Troubleshooting

### If openDevice() returns null after Option 1

The device may require explicit permission. Check if:
1. The device matches `device_filter.xml` criteria
2. The intent action is `USB_DEVICE_ATTACHED`

Add diagnostic logging:
```rust
// Before openDevice call
let device_name = env.call_method(&device, "getDeviceName", "()Ljava/lang/String;", &[])
    .ok()?.l().ok()?;
let name_str = env.get_string(&device_name.into()).ok()?.into();
log::info!("Opening device: {}", name_str);
```

### If no devices found in getDeviceList()

The USB system may not have enumerated the device yet. Add a retry with delay:
```rust
for _ in 0..3 {
    if let Some(device) = get_first_device_from_manager(...) {
        return Some(device);
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    log::info!("Retrying device enumeration...");
}
```

### If libusb fails to wrap fd

The file descriptor may be invalid. Verify:
```rust
log::info!("fd validity check: {}", fd >= 0);
```
