<script lang="ts">
import type { ConnectionStatus } from "./types";

const {
  connectionStatus,
  disconnectReason,
  reconnectAttempt,
  reconnectMessage,
}: {
  connectionStatus: ConnectionStatus;
  disconnectReason: string | null;
  reconnectAttempt: number;
  reconnectMessage: string | null;
} = $props();
</script>

{#if connectionStatus === "disconnected" || connectionStatus === "reconnecting"}
  <div class="overlay">
    <div class="waiting-message">
      {#if connectionStatus === "reconnecting"}
        <div class="camera-icon reconnecting">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M23 4v6h-6"/>
            <path d="M1 20v-6h6"/>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/>
          </svg>
        </div>
        <p>Reconnecting...</p>
        <span class="hint">
          Attempt {reconnectAttempt}
          {#if reconnectMessage}
            <br/>{reconnectMessage}
          {/if}
        </span>
      {:else if disconnectReason === "device_unplugged"}
        <div class="camera-icon error">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/>
            <line x1="1" y1="1" x2="23" y2="23"/>
          </svg>
        </div>
        <p>Camera Disconnected</p>
        <span class="hint">The USB camera was unplugged. Reconnect to continue.</span>
      {:else if disconnectReason === "timeout"}
        <div class="camera-icon warning">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/>
            <polyline points="12,6 12,12 16,14"/>
          </svg>
        </div>
        <p>Connection Timeout</p>
        <span class="hint">No video frames received. Check camera connection.</span>
      {:else if disconnectReason === "transfer_error"}
        <div class="camera-icon error">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/>
            <line x1="12" y1="8" x2="12" y2="12"/>
            <line x1="12" y1="16" x2="12.01" y2="16"/>
          </svg>
        </div>
        <p>USB Transfer Error</p>
        <span class="hint">A USB communication error occurred. Try reconnecting the camera.</span>
      {:else}
        <div class="camera-icon">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/>
            <circle cx="12" cy="13" r="4"/>
          </svg>
        </div>
        <p>Connect USB Endoscope</p>
        <span class="hint">Plug in your USB-C camera to start</span>
      {/if}
    </div>
  </div>
{/if}

<style>
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

  .camera-icon.error {
    color: #f87171;
  }

  .camera-icon.warning {
    color: #fbbf24;
  }

  .camera-icon.reconnecting {
    color: #60a5fa;
    animation: spin 2s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
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
</style>
