<script lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { onDestroy, onMount } from "svelte";
// biome-ignore lint/correctness/noUnusedImports: used in Svelte template
import CaptureInfo from "./lib/CaptureInfo.svelte";
// biome-ignore lint/correctness/noUnusedImports: used in Svelte template
import ConnectionOverlay from "./lib/ConnectionOverlay.svelte";
// biome-ignore lint/correctness/noUnusedImports: used in Svelte template
import DebugControls from "./lib/DebugControls.svelte";
// biome-ignore lint/correctness/noUnusedImports: used in Svelte template
import StatusBar from "./lib/StatusBar.svelte";
import type {
  BuildInfo,
  CaptureResult,
  ConnectionStatus,
  ReconnectStatus,
  ResolutionInfo,
  UsbError,
  UsbStatusExtended,
} from "./lib/types";

let connectionStatus = $state<ConnectionStatus>("disconnected");
let cameraInfo = $state<string>("");
let currentResolution = $state<string>("");
let resolutionInfo = $state<ResolutionInfo | null>(null);
let isCyclingResolution = $state<boolean>(false);
let errorMessage = $state<string>("");
let lastUsbError = $state<UsbError | null>(null);
let disconnectReason = $state<string | null>(null);
let reconnectAttempt = $state<number>(0);
let reconnectMessage = $state<string | null>(null);
let frameCount = $state<number>(0);
let buildInfo = $state<BuildInfo | null>(null);
let captureResult = $state<CaptureResult | null>(null);

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
const FPS_SAMPLE_SIZE = 30;
let frameTimestamps = $state<number[]>([]);
const currentFps = $derived.by(() => {
  if (frameTimestamps.length < 2) return 0;
  const oldestTimestamp = frameTimestamps[0];
  const newestTimestamp = frameTimestamps[frameTimestamps.length - 1];
  const timeSpanMs = newestTimestamp - oldestTimestamp;
  if (timeSpanMs <= 0) return 0;
  return Math.round(((frameTimestamps.length - 1) / timeSpanMs) * 1000);
});

// Curated color palette - distinct, visible on dark backgrounds
const BUILD_COLORS = [
  "#f87171",
  "#fb923c",
  "#fbbf24",
  "#a3e635",
  "#4ade80",
  "#2dd4bf",
  "#22d3ee",
  "#60a5fa",
  "#a78bfa",
  "#f472b6",
  "#e879f9",
  "#c084fc",
];

function hashToColorIndex(hash: string): number {
  const hexValue = parseInt(hash.slice(0, 6), 16);
  return hexValue % BUILD_COLORS.length;
}

const buildColor = $derived.by(() => {
  if (!buildInfo?.git_hash) return "#9ca3af";
  return BUILD_COLORS[hashToColorIndex(buildInfo.git_hash)];
});

const displayStatus = $derived.by(() => {
  if (connectionStatus === "disconnected") {
    return wasConnected ? "Connection lost" : "Waiting for device...";
  }
  if (connectionStatus === "reconnecting") {
    return `Reconnecting (attempt ${reconnectAttempt})...`;
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

// Timer cleanup
let resolutionTimeoutId: ReturnType<typeof setTimeout> | null = null;

// Rendering backpressure guard
let rendering = false;

onMount(async () => {
  if (canvas) {
    ctx = canvas.getContext("2d");
  }

  const unlistenUsb = await listen<UsbStatusExtended>("usb-device-event", async (event) => {
    if (event.payload.connected) {
      connectionStatus = "connected";
      cameraInfo = event.payload.info || "USB Camera";
      wasConnected = true;
      lastUsbError = null;
      disconnectReason = null;
      resolutionTimeoutId = setTimeout(() => fetchCurrentResolution(), 500);
    } else {
      connectionStatus = "disconnected";
      cameraInfo = "";
      frameCount = 0;
      frameTimestamps = [];
      resolutionInfo = null;
      disconnectReason = event.payload.disconnect_reason || null;
    }
  });
  unlistenFns.push(unlistenUsb);

  const unlistenUsbError = await listen<UsbError>("usb-error", (event) => {
    console.log("USB error received:", event.payload);
    lastUsbError = event.payload;
    errorMessage = event.payload.message;
  });
  unlistenFns.push(unlistenUsbError);

  const unlistenReconnect = await listen<ReconnectStatus>("usb-reconnecting", (event) => {
    console.log("Reconnection status:", event.payload);
    if (event.payload.reconnecting) {
      connectionStatus = "reconnecting";
      reconnectAttempt = event.payload.attempt;
      reconnectMessage = event.payload.message || null;
    } else {
      reconnectAttempt = 0;
      reconnectMessage = null;
    }
  });
  unlistenFns.push(unlistenReconnect);

  const unlistenUsbStatus = await listen<{ status: string; detail?: string }>(
    "usb-status",
    (event) => {
      streamingStatus = event.payload.detail || event.payload.status;
      if (event.payload.status === "connecting") {
        connectionStatus = "connecting";
      } else if (event.payload.status === "error") {
        errorMessage = event.payload.detail || "USB error occurred";
      }
    },
  );
  unlistenFns.push(unlistenUsbStatus);

  const unlistenFrame = await listen<{ width: number; height: number; format: string } | null>(
    "frame-ready",
    async (event) => {
      if (rendering) return;
      try {
        rendering = true;
        const frameInfoPromise = event.payload?.width
          ? Promise.resolve(event.payload)
          : invoke<{ width: number; height: number; format: string }>("get_frame_info");

        const [frameInfo, frameData] = await Promise.all([
          frameInfoPromise,
          invoke<ArrayBuffer>("get_frame"),
        ]);

        await renderFrame(frameData, frameInfo.format, frameInfo.width, frameInfo.height);
        frameCount++;

        const now = performance.now();
        frameTimestamps = [...frameTimestamps.slice(-(FPS_SAMPLE_SIZE - 1)), now];
      } catch (e) {
        console.debug("Frame fetch error:", e);
      } finally {
        rendering = false;
      }
    },
  );
  unlistenFns.push(unlistenFrame);

  try {
    const status = await invoke<{ connected: boolean; info?: string }>("check_usb_status");
    if (status.connected) {
      connectionStatus = "connected";
      cameraInfo = status.info || "USB Camera";
    }
  } catch (e) {
    console.log("No USB device on startup");
  }

  try {
    buildInfo = await invoke<BuildInfo>("get_build_info");
  } catch (e) {
    console.log("Could not get build info:", e);
  }

  try {
    videoFormatSetting = await invoke<string>("get_video_format");
  } catch (e) {
    console.log("Could not get video format:", e);
  }
});

onDestroy(() => {
  for (const unlisten of unlistenFns) {
    unlisten();
  }
  if (resolutionTimeoutId !== null) {
    clearTimeout(resolutionTimeoutId);
  }
  ctx = null;
});

async function renderFrame(
  data: ArrayBuffer,
  format: string,
  width: number,
  height: number,
): Promise<void> {
  if (!ctx || !canvas) return;

  if (format === "jpeg") {
    const blob = new Blob([data], { type: "image/jpeg" });
    const bitmap = await createImageBitmap(blob);

    if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      currentResolution = `${bitmap.width}x${bitmap.height}`;
    }

    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
  } else {
    const rgb = new Uint8Array(data);
    const expectedSize = width * height * 3;

    if (rgb.length < expectedSize) {
      console.debug(`RGB frame too small: ${rgb.length} < ${expectedSize}`);
      return;
    }

    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
      currentResolution = `${width}x${height}`;
    }

    const imageData = ctx.createImageData(width, height);
    const rgba = imageData.data;
    const pixelCount = width * height;

    for (let i = 0; i < pixelCount; i++) {
      const rgbIdx = i * 3;
      const rgbaIdx = i * 4;
      rgba[rgbaIdx] = rgb[rgbIdx];
      rgba[rgbaIdx + 1] = rgb[rgbIdx + 1];
      rgba[rgbaIdx + 2] = rgb[rgbIdx + 2];
      rgba[rgbaIdx + 3] = 255;
    }

    ctx.putImageData(imageData, 0, 0);
  }
}

async function cycleResolution() {
  if (connectionStatus !== "connected" || isCyclingResolution) return;
  isCyclingResolution = true;

  try {
    const info = await invoke<ResolutionInfo>("cycle_resolution");
    resolutionInfo = info;
    currentResolution = `${info.width}x${info.height}`;
  } catch (e) {
    console.error("Resolution cycle failed:", e);
    errorMessage = `Failed to change resolution: ${e}`;
  } finally {
    isCyclingResolution = false;
  }
}

async function fetchCurrentResolution() {
  try {
    const info = await invoke<ResolutionInfo>("get_current_resolution");
    resolutionInfo = info;
    currentResolution = `${info.width}x${info.height}`;
  } catch (e) {
    console.debug("Could not get current resolution:", e);
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
    captureResult = await invoke<CaptureResult>("dump_frame");
    console.log("Frame captured:", captureResult);
  } catch (e) {
    errorMessage = `Failed to capture frame: ${e}`;
  }
}
</script>

<main>
  <div class="container">
    <!-- Video Display Area -->
    <div class="video-container">
      <canvas bind:this={canvas} id="camera-canvas"></canvas>
      <ConnectionOverlay
        {connectionStatus}
        {disconnectReason}
        {reconnectAttempt}
        {reconnectMessage}
      />
    </div>

    <StatusBar
      {connectionStatus}
      {displayStatus}
      {cameraInfo}
      {frameCount}
      {buildInfo}
      {buildColor}
    />

    <DebugControls
      {widthSetting}
      {heightSetting}
      {strideSetting}
      {videoFormatSetting}
      {mjpegSetting}
      {pixelFormatSetting}
      {currentResolution}
      {resolutionInfo}
      {connectionStatus}
      {isCyclingResolution}
      oncyclewidth={cycleWidth}
      oncycleheight={cycleHeight}
      oncyclestride={cycleStride}
      oncyclevideoformat={cycleVideoFormat}
      ontogglemjpeg={toggleMjpeg}
      oncyclepixelformat={cyclePixelFormat}
      oncapture={captureFrame}
      oncycleresolution={cycleResolution}
    />

    {#if captureResult}
      <CaptureInfo
        {captureResult}
        ondismiss={() => captureResult = null}
      />
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
