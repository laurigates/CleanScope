---
paths:
  - "src/**/*.svelte"
  - "src/**/*.ts"
---

# Svelte Frontend Rules

## Overview

Rules for Svelte 5 frontend code. Applies to all `.svelte` and `.ts` files in `src/`.

## Svelte 5 Runes

Use runes syntax for reactivity (not legacy `$:` syntax):

```svelte
<script lang="ts">
  // State
  let count = $state(0);

  // Derived values
  let doubled = $derived(count * 2);

  // Effects
  $effect(() => {
    console.log('Count changed:', count);
  });
</script>
```

Avoid mixing runes with legacy reactive statements.

## Tauri Integration

### Invoking Commands

Use typed invoke calls:

```typescript
import { invoke } from '@tauri-apps/api/core';

// Type the response
const frame = await invoke<Uint8Array>('get_frame');

// With arguments
await invoke('set_display_settings', {
  width: 640,
  height: 480
});
```

### Listening to Events

Use cleanup pattern for event listeners:

```svelte
<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  onMount(() => {
    const unlisten = listen('frame-ready', (event) => {
      handleFrame(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  });
</script>
```

## Canvas Rendering

Use `createImageBitmap()` for efficient frame rendering:

```typescript
async function renderFrame(data: Uint8Array, width: number, height: number) {
  const imageData = new ImageData(
    new Uint8ClampedArray(data),
    width,
    height
  );
  const bitmap = await createImageBitmap(imageData);
  ctx.drawImage(bitmap, 0, 0);
  bitmap.close(); // Release memory
}
```

## TypeScript

Prefer explicit types over `any`:

```typescript
// Good
interface FramePayload {
  width: number;
  height: number;
  data: number[];
}

// Avoid
const payload: any = event.payload;
```

Use strict null checks. Handle undefined/null explicitly.

## Component Structure

Follow this order in `.svelte` files:

1. `<script lang="ts">` - Logic, imports, state
2. Markup - HTML template
3. `<style>` - Scoped styles (optional)

Keep components focused. Extract reusable logic into `.ts` files.
