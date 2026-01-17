# Work Order: Resolution Cycling UI

## Status: Ready for Testing
**Created:** 2026-01-17
**Priority:** Medium
**Feature:** F014
**Dependencies:** ADR-003

---

## Problem Statement

CleanScope currently uses a hardcoded `frame_index = 1` when negotiating UVC streaming, which always selects the first available resolution. Users cannot change video resolution to balance quality vs. performance.

### Current Behavior

1. App discovers available formats (MJPEG, YUY2) with resolution strings
2. UVC probe/commit always sends `b_frame_index = 1`
3. No UI to change resolution within a format
4. Format cycling exists but resolution cycling does not

### Desired Behavior (per ADR-003)

1. User can cycle through available resolutions
2. Brief stream interruption during change (~100-500ms)
3. Resolution preference persisted across sessions
4. Graceful fallback if negotiation fails

---

## Root Cause Analysis

### Code Location

File: `src-tauri/src/usb.rs`

**Line 1598** (approx): Frame index hardcoded
```rust
ctrl.b_frame_index = 1;
```

**Lines 434-474**: `DiscoveredFormat` stores resolution strings but not frame indices
```rust
pub struct DiscoveredFormat {
    pub index: u8,
    pub format_type: String,
    pub resolutions: Vec<String>,  // Only strings, no frame indices
}
```

**Lines 604-641**: Restart mechanism exists (`restart_requested` flag) - can be reused

---

## Implementation Plan

### Step 1: Extend Data Structures

**File:** `src-tauri/src/usb.rs`

Add `DiscoveredFrame` struct near `DiscoveredFormat`:

```rust
/// A discovered frame descriptor with resolution info
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredFrame {
    /// UVC frame index (1-based)
    pub frame_index: u8,
    /// Frame width in pixels
    pub width: u16,
    /// Frame height in pixels
    pub height: u16,
}
```

Modify `DiscoveredFormat`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredFormat {
    pub index: u8,
    pub format_type: String,
    pub frames: Vec<DiscoveredFrame>,  // Changed from resolutions: Vec<String>
}
```

### Step 2: Update Format Discovery

**File:** `src-tauri/src/usb.rs`

In `get_format_descriptors()` or similar discovery function, populate `frames` with `DiscoveredFrame` structs instead of resolution strings:

```rust
// When iterating through frame descriptors
let frame = DiscoveredFrame {
    frame_index: descriptor.b_frame_index,
    width: descriptor.w_width,
    height: descriptor.w_height,
};
format.frames.push(frame);
```

### Step 3: Add State for Selected Frame

**File:** `src-tauri/src/usb.rs`

Add to `StreamingConfig`:

```rust
pub struct StreamingConfig {
    // ... existing fields ...
    /// Selected frame index for resolution (None = use first available)
    pub selected_frame_index: Option<u8>,
    /// Currently active resolution info
    pub current_resolution: Option<(u16, u16)>,  // (width, height)
}
```

### Step 4: Make Probe/Commit Dynamic

**File:** `src-tauri/src/usb.rs`

In `start_uvc_streaming_with_resolution()` or UVC negotiation function:

```rust
// Before (hardcoded):
ctrl.b_frame_index = 1;

// After (dynamic):
let frame_index = config.selected_frame_index.unwrap_or(1);
ctrl.b_frame_index = frame_index;
```

### Step 5: Add Tauri Commands

**File:** `src-tauri/src/lib.rs`

Add return type:

```rust
#[derive(Clone, Serialize)]
pub struct ResolutionInfo {
    pub width: u16,
    pub height: u16,
    pub frame_index: u8,
    pub available_count: usize,
}
```

Add `cycle_resolution` command:

```rust
#[tauri::command]
async fn cycle_resolution(state: State<'_, AppState>) -> Result<ResolutionInfo, String> {
    let mut config = state.streaming_config.lock().map_err(|e| e.to_string())?;

    // Get current format's available frames
    let current_format_idx = config.selected_format_index.unwrap_or(1);
    let frames = config.available_formats
        .iter()
        .find(|f| f.index == current_format_idx)
        .map(|f| &f.frames)
        .ok_or_else(|| "No format selected".to_string())?;

    if frames.is_empty() {
        return Err("No resolutions available".to_string());
    }

    // Get current frame index or default to first
    let current_idx = config.selected_frame_index.unwrap_or(frames[0].frame_index);

    // Find next frame index (cycle through)
    let current_pos = frames.iter().position(|f| f.frame_index == current_idx).unwrap_or(0);
    let next_pos = (current_pos + 1) % frames.len();
    let next_frame = &frames[next_pos];

    // Update state
    config.selected_frame_index = Some(next_frame.frame_index);
    config.restart_requested = true;

    log::info!("Cycling resolution to {}x{} (frame_index={})",
        next_frame.width, next_frame.height, next_frame.frame_index);

    Ok(ResolutionInfo {
        width: next_frame.width,
        height: next_frame.height,
        frame_index: next_frame.frame_index,
        available_count: frames.len(),
    })
}
```

Add `get_current_resolution` command:

```rust
#[tauri::command]
fn get_current_resolution(state: State<'_, AppState>) -> Result<ResolutionInfo, String> {
    let config = state.streaming_config.lock().map_err(|e| e.to_string())?;

    let current_format_idx = config.selected_format_index.unwrap_or(1);
    let frames = config.available_formats
        .iter()
        .find(|f| f.index == current_format_idx)
        .map(|f| &f.frames)
        .ok_or_else(|| "No format selected".to_string())?;

    let current_idx = config.selected_frame_index.unwrap_or(1);
    let current_frame = frames.iter()
        .find(|f| f.frame_index == current_idx)
        .or_else(|| frames.first())
        .ok_or_else(|| "No frames available".to_string())?;

    Ok(ResolutionInfo {
        width: current_frame.width,
        height: current_frame.height,
        frame_index: current_frame.frame_index,
        available_count: frames.len(),
    })
}
```

Register in `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing handlers ...
    cycle_resolution,
    get_current_resolution,
])
```

### Step 6: Add Frontend UI

**File:** `src/App.svelte`

Add state variables:

```svelte
<script lang="ts">
    // ... existing imports ...

    interface ResolutionInfo {
        width: number;
        height: number;
        frame_index: number;
        available_count: number;
    }

    let currentResolution = $state<ResolutionInfo | null>(null);
    let isCyclingResolution = $state(false);
</script>
```

Add resolution cycling function:

```svelte
<script lang="ts">
    async function cycleResolution() {
        if (isCyclingResolution) return;
        isCyclingResolution = true;

        try {
            currentResolution = await invoke("cycle_resolution");
        } catch (e) {
            console.error("Resolution cycle failed:", e);
        } finally {
            isCyclingResolution = false;
        }
    }

    async function fetchCurrentResolution() {
        try {
            currentResolution = await invoke("get_current_resolution");
        } catch (e) {
            console.error("Failed to get resolution:", e);
        }
    }

    // Call on mount and after format changes
    onMount(() => {
        fetchCurrentResolution();
    });
</script>
```

Add UI elements (near existing format button):

```svelte
<div class="controls">
    <!-- Existing format button -->
    <button onclick={cycleVideoFormat} disabled={!isStreaming}>
        {videoFormat}
    </button>

    <!-- New resolution button -->
    <button
        onclick={cycleResolution}
        disabled={!isStreaming || isCyclingResolution}
        title="Cycle through available resolutions"
    >
        {#if currentResolution}
            {currentResolution.width}×{currentResolution.height}
        {:else}
            Resolution
        {/if}
    </button>

    <!-- Show available count -->
    {#if currentResolution && currentResolution.available_count > 1}
        <span class="resolution-hint">
            ({currentResolution.available_count} available)
        </span>
    {/if}
</div>
```

Add styles:

```svelte
<style>
    .resolution-hint {
        font-size: 0.75rem;
        color: #888;
        margin-left: 0.5rem;
    }
</style>
```

---

## Files to Modify

| File | Changes |
|------|---------|
| `src-tauri/src/usb.rs` | Add `DiscoveredFrame`, modify `DiscoveredFormat`, dynamic frame_index in probe |
| `src-tauri/src/lib.rs` | Add `ResolutionInfo`, `cycle_resolution`, `get_current_resolution` commands |
| `src/App.svelte` | Add resolution cycling UI, state, and handlers |

## Files for Reference (Do Not Modify)

| File | Purpose |
|------|---------|
| `docs/adrs/003-resolution-negotiation-protocol.md` | ADR defining the approach |
| `src-tauri/src/libusb_android.rs` | Low-level USB transfers (unchanged) |

---

## Testing Procedure

### Build and Deploy

```bash
just android-build
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
```

### Test Steps

1. **Connect endoscope, start streaming**
   ```bash
   adb logcat -s RustStdoutStderr:* | grep -E "resolution|frame_index|Cycling"
   ```

2. **Tap resolution button multiple times**
   - Should see resolution change (e.g., 640x480 → 1280x720 → 1920x1080 → 640x480)
   - Brief stream pause during negotiation
   - Video should resume at new resolution

3. **Verify logs show negotiation**
   ```
   Cycling resolution to 1280x720 (frame_index=2)
   UVC Probe sent with frame_index=2
   UVC negotiated: 1280x720
   ```

4. **Test edge cases**
   - Single resolution format (button shows count=1)
   - Rapid clicking (debounced, no double-cycling)
   - Resolution change during active stream

### Expected Success Logs

```
Found format MJPEG with 3 frames
  Frame 1: 640x480
  Frame 2: 1280x720
  Frame 3: 1920x1080
Cycling resolution to 1280x720 (frame_index=2)
Restart requested, stopping stream
UVC streaming restarted with frame_index=2
```

---

## Acceptance Criteria

- [ ] Resolution button displays current resolution (WxH)
- [ ] Clicking button cycles to next available resolution
- [ ] Video stream restarts at new resolution
- [ ] Multiple resolutions are discovered from UVC descriptors
- [ ] Works with both MJPEG and YUY2 formats
- [ ] No crash on rapid button clicks
- [ ] Logs show frame_index changing in probe/commit

---

## Future Enhancements (Out of Scope)

Per ADR-003:
- **Phase 2**: Show resolution picker dropdown instead of cycling
- **Phase 3**: Add quality presets (Low Power, Balanced, High Quality)
- **Persistence**: Save preferred resolution to app data dir

---

## Troubleshooting

### Resolution button shows single option

Some cheap endoscopes only support one resolution per format. Check descriptor dump:
```bash
adb logcat -s RustStdoutStderr:* | grep "Frame"
```

### Stream doesn't restart after resolution change

Check `restart_requested` flag is being set and the streaming loop is checking it:
```bash
adb logcat -s RustStdoutStderr:* | grep -E "restart|Restart"
```

### Negotiation fails (STALL)

Camera rejected the frame_index. May need fallback logic:
```rust
// If probe fails, try frame_index=1 as fallback
if result.is_err() {
    config.selected_frame_index = Some(1);
    // retry...
}
```

---

## Related Documentation

- [ADR-003: Resolution Negotiation Protocol](../adrs/003-resolution-negotiation-protocol.md)
- [ADR-001: Frame Streaming Architecture](../adrs/001-frame-streaming-architecture.md)
- [USB Video Class 1.5 Specification](https://www.usb.org/document-library/video-class-v15-document-set)
