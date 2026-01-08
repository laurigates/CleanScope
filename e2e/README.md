# CleanScope E2E Tests

End-to-end tests for the CleanScope Tauri application using Playwright.

## Overview

These tests verify the web UI functionality, including:
- App launch and initialization
- Frame rendering from USB replay data
- Resolution display and updates
- Status indicator and connection state
- Debug controls functionality

## Running Tests

### Headless (CI mode)

```bash
npm run test:e2e
```

### With Browser UI (development)

```bash
npm run test:e2e:headed
```

### Watch Mode

```bash
npx playwright test --watch
```

### Run Specific Test

```bash
npx playwright test frame-streaming.spec.ts
```

### Debug Mode

```bash
npx playwright test --debug
```

## Replay Mode

Tests can use USB packet replay instead of physical hardware by setting `CLEANSCOPE_REPLAY_PATH`:

```bash
CLEANSCOPE_REPLAY_PATH=tests/fixtures/sample-replay.bin npm run test:e2e
```

The replay path should point to a binary file in the capture format:
```
[u64 LE: timestamp_us][u32 LE: length][u8: endpoint][data bytes]...
```

### Creating Replay Fixtures

1. **Capture real data** using the app with a physical endoscope:
   ```bash
   # On Android device with ADB
   adb shell run-as com.cleanscope.app ls -la /data/files/
   adb shell run-as com.cleanscope.app cat /data/files/capture_*.bin > capture.bin
   ```

2. **Use existing fixtures** - the test suite creates a minimal MJPEG sample if none exists

3. **Format validation** - ensure the binary file starts with valid timestamps and packet data

## Architecture

- `playwright.config.ts` - Configuration file at project root (starts Tauri dev server)
- `e2e/frame-streaming.spec.ts` - Main test suite for frame rendering and UI

### How It Works

1. Playwright starts the Tauri dev server (`npm run tauri:dev`)
2. Tests connect to the web content at `http://localhost:1420`
3. If `CLEANSCOPE_REPLAY_PATH` is set, the Rust backend loads replay packets instead of USB
4. Tests verify UI elements and frame rendering behavior

## Fixtures

Test fixtures are in `tests/fixtures/`:

- `sample-replay.bin` - Auto-generated minimal MJPEG frame (2x2 pixel)
- Custom fixtures should be placed here and referenced in tests

## CI/CD Integration

Add to your CI pipeline:

```yaml
# .github/workflows/test.yml
- name: Run E2E tests
  env:
    CLEANSCOPE_REPLAY_PATH: tests/fixtures/sample-replay.bin
  run: npm run test:e2e
```

## Limitations

- **USB Hardware**: Tests use replay mode and don't require physical USB devices
- **Android**: Tests run on desktop only (E2E testing Android requires additional setup)
- **Real-time Behavior**: Replay timing may not match real hardware exactly

## Troubleshooting

### Port Already in Use (1420)

```bash
# Kill existing Tauri dev process
pkill -f "tauri dev"
# Or change port in vite.config.ts
```

### Replay File Not Found

The test suite auto-generates a sample fixture. To use custom data:
1. Capture frames with the running app
2. Place `.bin` file in `tests/fixtures/`
3. Set `CLEANSCOPE_REPLAY_PATH` environment variable

### Tests Timeout

Increase timeout in `playwright.config.ts` under `webServer`:
```typescript
timeout: 180 * 1000,  // 3 minutes
```

## Documentation

- [Playwright Documentation](https://playwright.dev/)
- [Tauri with Playwright](https://tauri.app/v1/guides/testing/e2e/playwright/)
- CleanScope Architecture: See main `CLAUDE.md`
