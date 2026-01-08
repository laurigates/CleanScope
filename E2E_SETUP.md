# Playwright E2E Testing Setup for CleanScope

This guide explains the E2E testing infrastructure set up for the CleanScope Tauri application.

## What Was Installed

### 1. Dependencies Added to `package.json`
- `@playwright/test@^1.48.0` - Playwright testing framework

### 2. npm Scripts Added
```json
{
  "test:e2e": "playwright test",
  "test:e2e:headed": "playwright test --headed"
}
```

## Project Structure

```
cleanscope-tests/
├── playwright.config.ts          # Playwright configuration
├── e2e/
│   ├── frame-streaming.spec.ts   # Main test suite
│   ├── helpers.ts                # Test utility functions
│   └── README.md                 # Detailed E2E testing docs
├── tests/
│   └── fixtures/                 # Test data (auto-created)
│       └── sample-replay.bin      # Minimal MJPEG sample
└── .gitignore                    # Updated with test artifacts
```

## Configuration Details

### playwright.config.ts

Key settings:
- **Base URL**: `http://localhost:1420` (Tauri dev server)
- **Test Directory**: `./e2e`
- **webServer**: Automatically starts `npm run tauri:dev` before tests
- **Reporter**: HTML report with GitHub Actions support for CI
- **Screenshot**: Captured on failure for debugging
- **Trace**: Recorded on first retry for investigation
- **Browser**: Chromium (default, matches Tauri webview)

### Environment Variables

Tests respect the `CLEANSCOPE_REPLAY_PATH` environment variable:

```bash
# Use specific replay data
CLEANSCOPE_REPLAY_PATH=tests/fixtures/custom.bin npm run test:e2e

# Use auto-generated sample
npm run test:e2e
```

When set, the Rust backend loads USB packets from the file instead of requiring physical hardware.

## Test Suite: frame-streaming.spec.ts

Covers these scenarios:

### Basic Functionality
- **App launches successfully** - Verifies UI elements load
- **Displays waiting message** - Shows when no device connected
- **Status indicator visible** - Pulsing connection status dot
- **Canvas renders** - Video display element present

### Frame Rendering (requires replay data)
- **Frame rendering from replay** - Tests that frames load from `CLEANSCOPE_REPLAY_PATH`
- **Resolution display updates** - Shows detected resolution (e.g., "800x600")

### UI Controls
- **Debug controls present** - Width, Height, Stride, Offset, Capture buttons
- **Frame count increments** - Tracks incoming frames
- **Build info displays** - Shows version and git hash
- **Error banner dismisses** - Cleanup of error notifications

## Running Tests

### First Time Setup

Install dependencies:
```bash
npm install
```

### Run All Tests (Headless)

```bash
npm run test:e2e
```

Output:
- Test results in terminal
- HTML report: `playwright-report/index.html`
- Screenshots/traces: `test-results/`

### Run with Browser UI

Useful for debugging:
```bash
npm run test:e2e:headed
```

The browser window shows each step, allowing you to see what the test is doing.

### Run Specific Test

```bash
# Just the frame-streaming suite
npx playwright test frame-streaming

# Specific test by name
npx playwright test --grep "app launches successfully"
```

### Debug Mode

Interactive debugging with step-through:
```bash
npx playwright test --debug
```

Opens Inspector UI where you can:
- Step through each action
- Inspect elements
- Execute JavaScript in console

### Watch Mode (development)

Automatically rerun tests on file changes:
```bash
npx playwright test --watch
```

## Test Fixtures

### Auto-Generated Fixture

The first test run creates `tests/fixtures/sample-replay.bin` automatically:
- Minimal JPEG frame (2x2 pixels)
- Valid packet format for replay testing
- Sufficient for basic rendering tests

### Custom Fixtures

To add real endoscope data:

1. **Capture from device**:
   ```bash
   # With app running on physical Android device over ADB
   adb shell run-as com.cleanscope.app ls -la /data/files/
   adb shell run-as com.cleanscope.app cat /data/files/capture_*.bin > real.bin
   ```

2. **Place in fixtures directory**:
   ```bash
   cp real.bin tests/fixtures/
   ```

3. **Use in tests**:
   ```bash
   CLEANSCOPE_REPLAY_PATH=tests/fixtures/real.bin npm run test:e2e
   ```

### Fixture Format

Binary packet stream (from `replay.rs`):
```
[u64 LE: timestamp_us]
[u32 LE: packet_length]
[u8: USB_endpoint]
[packet_data: length bytes]
...
```

Example packet headers:
- `01 00 00 00 00 00 00 00` = timestamp 1 microsecond
- `42 01 00 00` = 322 bytes of data
- `82` = IN endpoint
- `[322 bytes of USB packet data]`

## Helper Functions (e2e/helpers.ts)

Utility functions for tests:

```typescript
// Connection status
await waitForAppReady(page);
await getConnectionStatus(page);
await isConnected(page);

// Display info
await getResolution(page);
await getFrameCount(page);
await getBuildInfo(page);

// Frame streaming
await waitForFrames(page, expectedCount, timeout);

// UI interaction
await clickDebugButton(page, "Capture");
await dismissError(page);

// Verification
await isWaitingForDevice(page);
await getCanvasInfo(page);
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: E2E Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust  # Rust setup (if needed)
      - run: npm install
      - name: Run E2E tests
        env:
          CLEANSCOPE_REPLAY_PATH: tests/fixtures/sample-replay.bin
        run: npm run test:e2e
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: playwright-report
          path: playwright-report/
```

## Troubleshooting

### Port 1420 Already in Use

```bash
# Find and kill existing process
lsof -i :1420 | grep -v COMMAND | awk '{print $2}' | xargs kill -9

# Or change port in vite.config.ts and playwright.config.ts
```

### Replay Path Not Found

The test auto-generates a sample fixture. To debug:
```bash
# Verify fixture exists
ls -la tests/fixtures/sample-replay.bin

# Check permissions
chmod 644 tests/fixtures/sample-replay.bin

# Regenerate if corrupt
rm tests/fixtures/sample-replay.bin
npm run test:e2e  # Auto-creates
```

### Tests Timeout

Increase timeout in `playwright.config.ts`:
```typescript
export default defineConfig({
  timeout: 30 * 1000,  // 30 seconds per test
  webServer: {
    timeout: 180 * 1000,  // 3 minutes to start server
  },
});
```

### Slow Test Execution

- Run in parallel: `npx playwright test --workers=4`
- Skip headed mode (use `npm run test:e2e` not `test:e2e:headed`)
- Use `--reporter=dot` for minimal output (faster CI)

## Next Steps

1. **Add integration tests** for specific USB device types
2. **Add performance tests** for frame streaming latency
3. **Add accessibility tests** for a11y compliance
4. **Integrate with CI/CD** pipeline
5. **Expand fixtures** with real endoscope capture data

## Resources

- [Playwright Documentation](https://playwright.dev/docs/intro)
- [Tauri Testing Guide](https://tauri.app/v1/guides/testing/e2e/playwright/)
- [CleanScope CLAUDE.md](./CLAUDE.md) - Project architecture
- [E2E Testing README](./e2e/README.md) - Detailed E2E docs

## Notes

- Tests run on desktop/web only (Android requires different setup)
- USB functionality is tested via replay mode, not live hardware
- Tests respect the app's privacy-first design (no permission prompts)
- Video rendering is verified via canvas inspection, not pixel comparison
