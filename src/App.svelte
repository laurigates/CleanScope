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
let captureResult = $state<{
  path: string;
  raw_path: string | null;
  size: number;
  raw_size: number;
  header_hex: string;
  format_hint: string;
  width: number;
  height: number;
} | null>(null);

// Display settings for debugging (width, height, stride)
let widthSetting = $state<string>("W:Auto");
let heightSetting = $state<string>("H:Auto");
let strideSetting = $state<string>("S:Auto");

// Streaming configuration (MJPEG skip, pixel format, video format)
let mjpegSetting = $state<string>("MJPEG:Try");
let pixelFormatSetting = $state<string>("FMT:YUYV");
let videoFormatSetting = $state<string>("FMT:Auto");

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

// Curated color palette - distinct, visible on dark backgrounds
// Colors chosen to be maximally distinguishable from each other
const BUILD_COLORS = [
  "#f87171", // red-400
  "#fb923c", // orange-400
  "#fbbf24", // amber-400
  "#a3e635", // lime-400
  "#4ade80", // green-400
  "#2dd4bf", // teal-400
  "#22d3ee", // cyan-400
  "#60a5fa", // blue-400
  "#a78bfa", // violet-400
  "#f472b6", // pink-400
  "#e879f9", // fuchsia-400
  "#c084fc", // purple-400
];

// Hash a git hash string to a color index
function hashToColorIndex(hash: string): number {
  // Use first 6 chars of hash, parse as hex, mod by palette size
  const hexValue = parseInt(hash.slice(0, 6), 16);
  return hexValue % BUILD_COLORS.length;
}

// Get color for current build
const buildColor = $derived.by(() => {
  if (!buildInfo?.git_hash) return "#9ca3af"; // gray fallback
  return BUILD_COLORS[hashToColorIndex(buildInfo.git_hash)];
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
      // Fetch frame info (format, dimensions) and raw bytes in parallel
      const [frameInfo, frameData] = await Promise.all([
        invoke<{ width: number; height: number; format: string }>("get_frame_info"),
        invoke<ArrayBuffer>("get_frame"),
      ]);

      await renderFrame(frameData, frameInfo.format, frameInfo.width, frameInfo.height);
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

  // Get initial video format setting
  try {
    videoFormatSetting = await invoke<string>("get_video_format");
  } catch (e) {
    console.log("Could not get video format:", e);
  }
});

onDestroy(() => {
  // Clean up event listeners
  for (const unlisten of unlistenFns) {
    unlisten();
  }
});

/**
 * Render a frame to the canvas
 * Supports both JPEG (from MJPEG cameras) and RGB24 (from YUY2 cameras)
 */
async function renderFrame(
  data: ArrayBuffer,
  format: string,
  width: number,
  height: number,
): Promise<void> {
  if (!ctx || !canvas) return;

  if (format === "jpeg") {
    // JPEG: Use browser-native decoding via createImageBitmap
    const blob = new Blob([data], { type: "image/jpeg" });
    const bitmap = await createImageBitmap(blob);

    // Resize canvas to match frame dimensions (only if changed)
    if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      currentResolution = `${bitmap.width}x${bitmap.height}`;
    }

    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
  } else {
    // RGB24: Convert to RGBA and use putImageData
    const rgb = new Uint8Array(data);
    const expectedSize = width * height * 3;

    if (rgb.length < expectedSize) {
      console.debug(`RGB frame too small: ${rgb.length} < ${expectedSize}`);
      return;
    }

    // Resize canvas if needed
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
      currentResolution = `${width}x${height}`;
    }

    // Convert RGB24 to RGBA32 (canvas requires alpha channel)
    const imageData = ctx.createImageData(width, height);
    const rgba = imageData.data;
    const pixelCount = width * height;

    for (let i = 0; i < pixelCount; i++) {
      const rgbIdx = i * 3;
      const rgbaIdx = i * 4;
      rgba[rgbaIdx] = rgb[rgbIdx]; // R
      rgba[rgbaIdx + 1] = rgb[rgbIdx + 1]; // G
      rgba[rgbaIdx + 2] = rgb[rgbIdx + 2]; // B
      rgba[rgbaIdx + 3] = 255; // A (fully opaque)
    }

    ctx.putImageData(imageData, 0, 0);
  }
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

async function cycleWidth() {
  try {
    widthSetting = await invoke<string>("cycle_width");
  } catch (e) {
    errorMessage = `Failed to change width: ${e}`;
  }
}

async function cycleHeight() {
  try {
    heightSetting = await invoke<string>("cycle_height");
  } catch (e) {
    errorMessage = `Failed to change height: ${e}`;
  }
}

async function cycleStride() {
  try {
    strideSetting = await invoke<string>("cycle_stride");
  } catch (e) {
    errorMessage = `Failed to change stride: ${e}`;
  }
}

async function toggleMjpeg() {
  try {
    mjpegSetting = await invoke<string>("toggle_skip_mjpeg");
  } catch (e) {
    errorMessage = `Failed to toggle MJPEG: ${e}`;
  }
}

async function cyclePixelFormat() {
  try {
    pixelFormatSetting = await invoke<string>("cycle_pixel_format");
  } catch (e) {
    errorMessage = `Failed to change pixel format: ${e}`;
  }
}

async function cycleVideoFormat() {
  try {
    videoFormatSetting = await invoke<string>("cycle_video_format");
  } catch (e) {
    errorMessage = `Failed to change video format: ${e}`;
  }
}

async function captureFrame() {
  try {
    captureResult = await invoke<{
      path: string;
      raw_path: string | null;
      size: number;
      raw_size: number;
      header_hex: string;
      format_hint: string;
      width: number;
      height: number;
    }>("dump_frame");
    console.log("Frame captured:", captureResult);
  } catch (e) {
    errorMessage = `Failed to capture frame: ${e}`;
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
          <span class="build-info" style="color: {buildColor}">v{buildInfo.version} ({buildInfo.git_hash})</span>
        {/if}
      </div>
    </div>

    <!-- Debug controls row -->
    <div class="debug-controls">
      <button class="debug-btn" onclick={cycleWidth}>{widthSetting}</button>
      <button class="debug-btn" onclick={cycleHeight}>{heightSetting}</button>
      <button class="debug-btn" onclick={cycleStride}>{strideSetting}</button>
      <button class="debug-btn format" onclick={cycleVideoFormat}>{videoFormatSetting}</button>
      <button class="debug-btn format" onclick={toggleMjpeg}>{mjpegSetting}</button>
      <button class="debug-btn format" onclick={cyclePixelFormat}>{pixelFormatSetting}</button>
      <button class="debug-btn capture" onclick={captureFrame}>Capture</button>
      {#if currentResolution}
        <button class="debug-btn detected">{currentResolution}</button>
      {/if}
    </div>

    {#if captureResult}
      <div class="capture-info">
        <div class="capture-header">
          <span class="capture-title">Captured Frame</span>
          <button class="dismiss-btn" onclick={() => captureResult = null}>×</button>
        </div>
        <div class="capture-details">
          <div><strong>Dimensions:</strong> {captureResult.width}x{captureResult.height}</div>
          <div><strong>Processed:</strong> {captureResult.size.toLocaleString()} bytes (RGB)</div>
          {#if captureResult.raw_size > 0}
            <div><strong>Raw:</strong> {captureResult.raw_size.toLocaleString()} bytes ({captureResult.format_hint})</div>
          {/if}
          <div class="hex-dump"><strong>Raw header:</strong> <code>{captureResult.header_hex}</code></div>
        </div>
        <div class="capture-hint">
          {#if captureResult.raw_path}
            <div>Raw: <code>adb shell run-as com.cleanscope.app cat "{captureResult.raw_path}" > raw.yuy2</code></div>
          {/if}
          <div>RGB: <code>adb shell run-as com.cleanscope.app cat "{captureResult.path}" > frame.rgb</code></div>
        </div>
      </div>
    {/if}

    {#if errorMessage}
      <div class="error-banner">
        {errorMessage}
        <button onclick={() => errorMessage = ""}>Dismiss</button>
      </div>
    {/if}
  </div>
</main>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    background: #0a0a0a;
    color: white;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    overflow: hidden;
    height: 100%;
    width: 100%;
  }

  main {
    position: fixed;
    inset: 0;
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
    color: #9ca3af;
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
    color: #9ca3af;
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
    color: #a1a1aa;
    margin-left: 0.5rem;
    padding-left: 0.5rem;
    border-left: 1px solid #444;
  }

  .frame-count {
    font-size: 0.75rem;
    color: #a1a1aa;
    font-family: monospace;
  }

  .status-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .build-info {
    font-size: 0.75rem;
    color: #9ca3af;
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

  .debug-controls {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    padding-bottom: max(0.75rem, env(safe-area-inset-bottom, 0px));
    background: #111;
    border-top: 1px solid #333;
    justify-content: center;
  }

  .debug-btn {
    background: #2563eb;
    border: none;
    color: white;
    padding: 0.75rem;
    border-radius: 8px;
    font-size: 0.75rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s cubic-bezier(0.4, 0, 0.2, 1);
    min-width: 48px;
    min-height: 48px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .debug-btn:hover {
    background: #3b82f6;
  }

  .debug-btn.detected {
    background: #059669;
  }

  .debug-btn.detected:hover {
    background: #10b981;
  }

  .debug-btn.format {
    background: #0891b2;
  }

  .debug-btn.format:hover {
    background: #06b6d4;
  }

  .debug-btn.capture {
    background: #7c3aed;
  }

  .debug-btn.capture:hover {
    background: #8b5cf6;
  }

  .capture-info {
    background: #1e1b4b;
    border: 1px solid #4c1d95;
    border-radius: 8px;
    padding: 0.75rem;
    margin: 0 1rem 0.5rem;
    font-size: 0.75rem;
  }

  .capture-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.5rem;
  }

  .capture-title {
    font-weight: 600;
    color: #a78bfa;
  }

  .dismiss-btn {
    background: transparent;
    border: none;
    color: #9ca3af;
    font-size: 1.25rem;
    cursor: pointer;
    padding: 0;
    line-height: 1;
  }

  .capture-details {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    color: #e2e8f0;
  }

  .capture-details code {
    background: #312e81;
    padding: 0.125rem 0.25rem;
    border-radius: 3px;
    font-size: 0.7rem;
    word-break: break-all;
  }

  .hex-dump code {
    display: block;
    margin-top: 0.25rem;
    padding: 0.375rem;
    font-family: monospace;
    overflow-x: auto;
    white-space: nowrap;
  }

  .capture-hint {
    margin-top: 0.5rem;
    padding-top: 0.5rem;
    border-top: 1px solid #4c1d95;
    color: #9ca3af;
    font-size: 0.7rem;
  }

  .capture-hint code {
    background: #312e81;
    padding: 0.125rem 0.25rem;
    border-radius: 3px;
  }

  .error-banner {
    position: fixed;
    bottom: calc(120px + env(safe-area-inset-bottom));
    left: max(1rem, env(safe-area-inset-left));
    right: max(1rem, env(safe-area-inset-right));
    background: #dc2626;
    color: white;
    padding: 0.75rem 1rem;
    border-radius: 8px;
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
