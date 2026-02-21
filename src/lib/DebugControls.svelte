<script lang="ts">
import type { ConnectionStatus, ResolutionInfo } from "./types";

const {
  widthSetting,
  heightSetting,
  strideSetting,
  videoFormatSetting,
  mjpegSetting,
  pixelFormatSetting,
  currentResolution,
  resolutionInfo,
  connectionStatus,
  isCyclingResolution,
  oncyclewidth,
  oncycleheight,
  oncyclestride,
  oncyclevideoformat,
  ontogglemjpeg,
  oncyclepixelformat,
  oncapture,
  oncycleresolution,
}: {
  widthSetting: string;
  heightSetting: string;
  strideSetting: string;
  videoFormatSetting: string;
  mjpegSetting: string;
  pixelFormatSetting: string;
  currentResolution: string;
  resolutionInfo: ResolutionInfo | null;
  connectionStatus: ConnectionStatus;
  isCyclingResolution: boolean;
  oncyclewidth: () => void;
  oncycleheight: () => void;
  oncyclestride: () => void;
  oncyclevideoformat: () => void;
  ontogglemjpeg: () => void;
  oncyclepixelformat: () => void;
  oncapture: () => void;
  oncycleresolution: () => void;
} = $props();
</script>

<div class="debug-controls">
  <button class="debug-btn" onclick={oncyclewidth}>{widthSetting}</button>
  <button class="debug-btn" onclick={oncycleheight}>{heightSetting}</button>
  <button class="debug-btn" onclick={oncyclestride}>{strideSetting}</button>
  <button class="debug-btn format" onclick={oncyclevideoformat}>{videoFormatSetting}</button>
  <button class="debug-btn format" onclick={ontogglemjpeg}>{mjpegSetting}</button>
  <button class="debug-btn format" onclick={oncyclepixelformat}>{pixelFormatSetting}</button>
  <button class="debug-btn capture" onclick={oncapture}>Capture</button>
  <button
    class="debug-btn resolution"
    onclick={oncycleresolution}
    disabled={connectionStatus !== "connected" || isCyclingResolution}
    title={resolutionInfo && resolutionInfo.available_count > 1
      ? `Tap to cycle (${resolutionInfo.available_count} available)`
      : "Resolution"}
  >
    {#if isCyclingResolution}
      Changing...
    {:else if currentResolution}
      {currentResolution}
      {#if resolutionInfo && resolutionInfo.available_count > 1}
        <span class="resolution-count">({resolutionInfo.available_count})</span>
      {/if}
    {:else}
      Resolution
    {/if}
  </button>
</div>

<style>
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

  .debug-btn.resolution {
    background: #059669;
  }

  .debug-btn.resolution:hover:not(:disabled) {
    background: #10b981;
  }

  .debug-btn.resolution:disabled {
    background: #374151;
    cursor: not-allowed;
    opacity: 0.7;
  }

  .resolution-count {
    font-size: 0.65rem;
    opacity: 0.8;
    margin-left: 0.25rem;
  }
</style>
