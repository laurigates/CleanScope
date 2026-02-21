<script lang="ts">
import type { BuildInfo, ConnectionStatus } from "./types";

const {
  connectionStatus,
  displayStatus,
  cameraInfo,
  frameCount,
  buildInfo,
  buildColor,
}: {
  connectionStatus: ConnectionStatus;
  displayStatus: string;
  cameraInfo: string;
  frameCount: number;
  buildInfo: BuildInfo | null;
  buildColor: string;
} = $props();

function getStatusColor(): string {
  switch (connectionStatus) {
    case "connected":
      return "#4ade80";
    case "connecting":
    case "reconnecting":
      return "#fbbf24";
    default:
      return "#ef4444";
  }
}
</script>

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

<style>
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
</style>
