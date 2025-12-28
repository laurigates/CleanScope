<script lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { onDestroy, onMount } from "svelte";

let connectionStatus = $state<"disconnected" | "connecting" | "connected">("disconnected");
let cameraInfo = $state<string>("");
let currentResolution = $state<string>("");
const availableResolutions = $state<string[]>([]);
let errorMessage = $state<string>("");
let frameCount = $state<number>(0);
let buildInfo = $state<{ version: string; git_hash: string; build_time: string } | null>(null);

// Streaming status for detailed feedback
let streamingStatus = $state<string>("Waiting for device...");
let wasConnected = $state<boolean>(false);

// FPS calculation - track timestamps of recent frames
const FPS_SAMPLE_SIZE = 30; // Number of frames to average over
let frameTimestamps = $state<number[]>([]);
const currentFps = $derived.by(() => {
  if (frameTimestamps.length < 2) return 0;
  const oldestTimestamp = frameTimestamps[0];
  const newestTimestamp = frameTimestamps[frameTimestamps.length - 1];
  const timeSpanMs = newestTimestamp - oldestTimestamp;
  if (timeSpanMs <= 0) return 0;
  // FPS = (number of intervals) / (time span in seconds)
  return Math.round(((frameTimestamps.length - 1) / timeSpanMs) * 1000);
});

// Derived streaming status message
const displayStatus = $derived.by(() => {
  if (connectionStatus === "disconnected") {
    return wasConnected ? "Connection lost" : "Waiting for device...";
  }
  if (connectionStatus === "connecting") {
    return "Device connected, initializing...";
  }
  if (connectionStatus === "connected") {
    if (frameCount === 0) {
      return "Connected, waiting for frames...";
    }
    return `Streaming (${currentFps} fps)`;
  }
  return streamingStatus;
});

// Canvas refs
let canvas: HTMLCanvasElement;
let ctx: CanvasRenderingContext2D | null = null;

// Event listener cleanup functions
const unlistenFns: UnlistenFn[] = [];

onMount(async () => {
  // Initialize canvas context
  if (canvas) {
    ctx = canvas.getContext("2d");
  }

  // Listen for USB device events from Rust backend
  const unlistenUsb = await listen<{ connected: boolean; info?: string }>(
    "usb-device-event",
    (event) => {
      if (event.payload.connected) {
        connectionStatus = "connected";
        cameraInfo = event.payload.info || "USB Camera";
        wasConnected = true;
      } else {
        connectionStatus = "disconnected";
        cameraInfo = "";
        frameCount = 0;
        frameTimestamps = [];
      }
    },
  );
  unlistenFns.push(unlistenUsb);

  // Listen for detailed USB status updates from Rust backend
  const unlistenUsbStatus = await listen<{ status: string; detail?: string }>(
    "usb-status",
    (event) => {
      streamingStatus = event.payload.detail || event.payload.status;
      // Update connection status based on status string
      if (event.payload.status === "connecting") {
        connectionStatus = "connecting";
      } else if (event.payload.status === "error") {
        errorMessage = event.payload.detail || "USB error occurred";
      }
    },
  );
  unlistenFns.push(unlistenUsbStatus);

  // Listen for frame-ready events and fetch frame data
  const unlistenFrame = await listen("frame-ready", async () => {
    try {
      // Fetch raw JPEG bytes from Rust backend
      const frameData: ArrayBuffer = await invoke("get_frame");
      await renderFrame(frameData);
      frameCount++;

      // Track timestamp for FPS calculation
      const now = performance.now();
      frameTimestamps = [...frameTimestamps.slice(-(FPS_SAMPLE_SIZE - 1)), now];
    } catch (e) {
      // Silently ignore frame fetch errors (e.g., no frame available yet)
      console.debug("Frame fetch error:", e);
    }
  });
  unlistenFns.push(unlistenFrame);

  // Check initial connection status
  try {
    const status = await invoke<{ connected: boolean; info?: string }>("check_usb_status");
    if (status.connected) {
      connectionStatus = "connected";
      cameraInfo = status.info || "USB Camera";
    }
  } catch (e) {
    console.log("No USB device on startup");
  }

  // Get build info
  try {
    buildInfo = await invoke<{ version: string; git_hash: string; build_time: string }>(
      "get_build_info",
    );
  } catch (e) {
    console.log("Could not get build info:", e);
  }
});

onDestroy(() => {
  // Clean up event listeners
  for (const unlisten of unlistenFns) {
    unlisten();
  }
});

/**
 * Render a JPEG frame to the canvas using browser-native decoding
 */
async function renderFrame(data: ArrayBuffer): Promise<void> {
  if (!ctx || !canvas) return;

  // Create blob from raw JPEG bytes
  const blob = new Blob([data], { type: "image/jpeg" });

  // Use createImageBitmap for efficient decoding (off main thread)
  const bitmap = await createImageBitmap(blob);

  // Resize canvas to match frame dimensions (only if changed)
  if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    currentResolution = `${bitmap.width}x${bitmap.height}`;
  }

  // Draw bitmap to canvas
  ctx.drawImage(bitmap, 0, 0);

  // Release bitmap resources
  bitmap.close();
}

async function cycleResolution() {
  if (connectionStatus !== "connected") return;
  try {
    const newRes = await invoke<string>("cycle_resolution");
    currentResolution = newRes;
  } catch (e) {
    errorMessage = `Failed to change resolution: ${e}`;
  }
}

function getStatusColor(): string {
  switch (connectionStatus) {
    case "connected":
      return "#4ade80";
    case "connecting":
      return "#fbbf24";
    default:
      return "#ef4444";
  }
}
</script>

<main>
  <div class="container">
    <!-- Video Display Area -->
    <div class="video-container">
      <canvas bind:this={canvas} id="camera-canvas"></canvas>
      {#if connectionStatus === "disconnected"}
        <div class="overlay">
          <div class="waiting-message">
            <div class="camera-icon">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/>
                <circle cx="12" cy="13" r="4"/>
              </svg>
            </div>
            <p>Connect USB Endoscope</p>
            <span class="hint">Plug in your USB-C camera to start</span>
          </div>
        </div>
      {/if}
    </div>

    <!-- Status Bar -->
    <div class="status-bar">
      <div class="status-indicator">
        <span class="dot" style="background-color: {getStatusColor()}"></span>
        <span class="status-text">
          {displayStatus}
        </span>
        {#if connectionStatus === "connected" && cameraInfo}
          <span class="camera-name">{cameraInfo}</span>
        {/if}
      </div>

      <div class="status-right">
        {#if frameCount > 0}
          <span class="frame-count">{frameCount.toLocaleString()} frames</span>
        {/if}
        {#if buildInfo}
          <span class="build-info">v{buildInfo.version} ({buildInfo.git_hash})</span>
        {/if}
        {#if currentResolution}
          <button class="resolution-btn" onclick={cycleResolution}>
            {currentResolution}
          </button>
        {/if}
      </div>
    </div>

    {#if errorMessage}
      <div class="error-banner">
        {errorMessage}
        <button onclick={() => errorMessage = ""}>Dismiss</button>
      </div>
    {/if}
  </div>
</main>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    background: #0a0a0a;
    color: white;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    overflow: hidden;
    height: 100vh;
    width: 100vw;
  }

  main {
    height: 100vh;
    width: 100vw;
    display: flex;
    flex-direction: column;
  }

  .container {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .video-container {
    flex: 1;
    position: relative;
    background: #111;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  #camera-canvas {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.8);
  }

  .waiting-message {
    text-align: center;
    padding: 2rem;
  }

  .camera-icon {
    width: 64px;
    height: 64px;
    margin: 0 auto 1rem;
    color: #666;
  }

  .camera-icon svg {
    width: 100%;
    height: 100%;
  }

  .waiting-message p {
    font-size: 1.25rem;
    margin: 0 0 0.5rem;
    color: #ccc;
  }

  .hint {
    font-size: 0.875rem;
    color: #666;
  }

  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1rem;
    background: #1a1a1a;
    border-top: 1px solid #333;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    animation: pulse 2s infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .status-text {
    font-size: 0.875rem;
  }

  .camera-name {
    font-size: 0.75rem;
    color: #888;
    margin-left: 0.5rem;
    padding-left: 0.5rem;
    border-left: 1px solid #444;
  }

  .frame-count {
    font-size: 0.75rem;
    color: #888;
    font-family: monospace;
  }

  .status-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .build-info {
    font-size: 0.7rem;
    color: #666;
    font-family: monospace;
  }

  .resolution-btn {
    background: #333;
    border: none;
    color: white;
    padding: 0.5rem 1rem;
    border-radius: 4px;
    font-size: 0.75rem;
    cursor: pointer;
    transition: background 0.2s;
  }

  .resolution-btn:hover {
    background: #444;
  }

  .error-banner {
    position: fixed;
    bottom: 60px;
    left: 1rem;
    right: 1rem;
    background: #dc2626;
    color: white;
    padding: 0.75rem 1rem;
    border-radius: 4px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.875rem;
  }

  .error-banner button {
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.5);
    color: white;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
    cursor: pointer;
  }
</style>
