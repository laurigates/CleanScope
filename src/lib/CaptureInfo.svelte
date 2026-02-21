<script lang="ts">
import type { CaptureResult } from "./types";

const {
  captureResult,
  ondismiss,
}: {
  captureResult: CaptureResult;
  ondismiss: () => void;
} = $props();
</script>

<div class="capture-info">
  <div class="capture-header">
    <span class="capture-title">Captured Frame</span>
    <button class="dismiss-btn" onclick={ondismiss}>×</button>
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

<style>
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
</style>
