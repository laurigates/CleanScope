# Frame Streaming via Tauri IPC

## Overview
Patterns for efficiently streaming binary video frame data from Rust backend to web frontend via Tauri's IPC.

## When to Use
- Streaming camera frames to the UI
- Sending binary data without Base64 overhead
- Implementing real-time video display
- Managing frame buffers in Rust

## Key Concepts

### ipc::Response for Binary Data
Tauri's `ipc::Response` sends raw bytes without Base64 encoding:
```rust
use tauri::ipc::Response;

#[tauri::command]
fn get_latest_frame() -> Response {
    let frame_data: Vec<u8> = get_frame_from_buffer();
    Response::new(frame_data)
}
```

### Polling vs Event Streaming

**Polling Pattern** (simpler, recommended for video):
```rust
// Backend: Command returns latest frame
#[tauri::command]
fn poll_frame(state: State<'_, FrameBuffer>) -> Response {
    let guard = state.0.lock().unwrap();
    Response::new(guard.clone())
}
```

**Event Pattern** (for notifications):
```rust
// Backend: Emit events for state changes
use tauri::Emitter;

fn notify_frame_available(app: &AppHandle) {
    app.emit("frame-available", ()).unwrap();
}
```

### Shared Frame Buffer
```rust
use std::sync::{Arc, Mutex};
use tauri::State;

pub struct FrameBuffer(pub Arc<Mutex<Vec<u8>>>);

#[tauri::command]
fn get_frame(state: State<'_, FrameBuffer>) -> Response {
    let guard = state.0.lock().unwrap();
    Response::new(guard.clone())
}

// In setup
fn main() {
    let frame_buffer = FrameBuffer(Arc::new(Mutex::new(Vec::new())));

    tauri::Builder::default()
        .manage(frame_buffer)
        .invoke_handler(tauri::generate_handler![get_frame])
        .run(tauri::generate_context!())
        .expect("error running app");
}
```

## Common Patterns

### Frame Producer (USB Thread)
```rust
fn start_frame_capture(
    buffer: Arc<Mutex<Vec<u8>>>,
    handle: DeviceHandle<Context>,
) {
    std::thread::spawn(move || {
        let mut raw_buffer = vec![0u8; 65536];

        loop {
            match handle.read_bulk(0x81, &mut raw_buffer, Duration::from_millis(100)) {
                Ok(bytes_read) => {
                    if let Some(frame) = extract_jpeg_frame(&raw_buffer[..bytes_read]) {
                        let mut guard = buffer.lock().unwrap();
                        *guard = frame;
                    }
                }
                Err(rusb::Error::Timeout) => continue,
                Err(e) => {
                    log::error!("USB read error: {}", e);
                    break;
                }
            }
        }
    });
}
```

### Frontend: Polling with requestAnimationFrame
```typescript
import { invoke } from "@tauri-apps/api/core";

let isStreaming = false;

async function pollFrame(): Promise<void> {
    if (!isStreaming) return;

    try {
        const frameData: ArrayBuffer = await invoke("get_frame");

        if (frameData.byteLength > 0) {
            await renderFrame(frameData);
        }
    } catch (error) {
        console.error("Frame poll error:", error);
    }

    requestAnimationFrame(pollFrame);
}

function startStreaming(): void {
    isStreaming = true;
    pollFrame();
}

function stopStreaming(): void {
    isStreaming = false;
}
```

### Rendering JPEG to Canvas
```typescript
async function renderFrame(data: ArrayBuffer): Promise<void> {
    const blob = new Blob([data], { type: "image/jpeg" });
    const bitmap = await createImageBitmap(blob);

    const canvas = document.getElementById("video-canvas") as HTMLCanvasElement;
    const ctx = canvas.getContext("2d")!;

    // Scale to fit canvas while maintaining aspect ratio
    const scale = Math.min(
        canvas.width / bitmap.width,
        canvas.height / bitmap.height
    );

    const x = (canvas.width - bitmap.width * scale) / 2;
    const y = (canvas.height - bitmap.height * scale) / 2;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(bitmap, x, y, bitmap.width * scale, bitmap.height * scale);

    bitmap.close();
}
```

### Alternative: Using img Element
```typescript
async function renderToImage(data: ArrayBuffer): Promise<void> {
    const blob = new Blob([data], { type: "image/jpeg" });
    const url = URL.createObjectURL(blob);

    const img = document.getElementById("video-img") as HTMLImageElement;

    // Clean up previous URL
    if (img.src.startsWith("blob:")) {
        URL.revokeObjectURL(img.src);
    }

    img.src = url;
}
```

### Event-Based Notification + Polling Hybrid
```rust
// Backend: Notify when new frame is available
fn frame_received(app: &AppHandle, frame: Vec<u8>) {
    let buffer = app.state::<FrameBuffer>();
    {
        let mut guard = buffer.0.lock().unwrap();
        *guard = frame;
    }
    app.emit("frame-ready", ()).unwrap();
}
```

```typescript
// Frontend: Listen for events, then poll
import { listen } from "@tauri-apps/api/event";

await listen("frame-ready", async () => {
    const frameData: ArrayBuffer = await invoke("get_frame");
    await renderFrame(frameData);
});
```

## Svelte 5 Integration

```svelte
<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { onMount, onDestroy } from "svelte";

    let canvas: HTMLCanvasElement;
    let isStreaming = $state(false);
    let frameCount = $state(0);

    async function pollFrame(): Promise<void> {
        if (!isStreaming) return;

        const frameData: ArrayBuffer = await invoke("get_frame");

        if (frameData.byteLength > 0) {
            const blob = new Blob([frameData], { type: "image/jpeg" });
            const bitmap = await createImageBitmap(blob);
            const ctx = canvas.getContext("2d")!;
            ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
            bitmap.close();
            frameCount++;
        }

        requestAnimationFrame(pollFrame);
    }

    function toggleStreaming(): void {
        isStreaming = !isStreaming;
        if (isStreaming) {
            pollFrame();
        }
    }

    onDestroy(() => {
        isStreaming = false;
    });
</script>

<canvas bind:this={canvas} width={640} height={480}></canvas>
<button onclick={toggleStreaming}>
    {isStreaming ? "Stop" : "Start"}
</button>
<p>Frames: {frameCount}</p>
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| No frames received | Check USB thread is writing to buffer |
| Frames choppy | Reduce frame processing time, use `requestAnimationFrame` |
| Memory leak | Call `bitmap.close()` and revoke blob URLs |
| Black frames | Verify JPEG SOI/EOI markers present |
| High latency | Use polling instead of events for video |

## References
- [Tauri IPC](https://v2.tauri.app/develop/calling-rust/)
- [createImageBitmap](https://developer.mozilla.org/en-US/docs/Web/API/createImageBitmap)
- [Canvas API](https://developer.mozilla.org/en-US/docs/Web/API/Canvas_API)
