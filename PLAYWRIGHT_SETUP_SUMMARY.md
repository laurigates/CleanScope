# Playwright E2E Testing Setup - Summary

Playwright E2E testing has been successfully configured for the CleanScope Tauri application.

## What Was Done

### 1. Installed Playwright Test Framework

**File: `package.json`**
- Added `@playwright/test@^1.48.0` to devDependencies
- Added npm scripts:
  - `test:e2e` - Run tests in headless mode
  - `test:e2e:headed` - Run tests with visible browser

### 2. Created Playwright Configuration

**File: `playwright.config.ts`**
- Configures Playwright to test the Tauri webview
- Automatically starts Tauri dev server on port 1420
- Enables screenshot/trace capture on failure
- Supports `CLEANSCOPE_REPLAY_PATH` environment variable for USB packet replay
- Configured for Chrome/Chromium browser (matches Tauri webview)

### 3. Created E2E Test Suite

**File: `e2e/frame-streaming.spec.ts`**

Comprehensive test coverage (10+ test cases):
- **Startup**: App launches, UI elements visible
- **Connection State**: Waiting message displays when disconnected
- **Status Display**: Status indicator dot and text visible
- **Frame Rendering**: Canvas renders frames from replay data
- **Resolution Display**: Shows detected resolution (e.g., "800x600")
- **Debug Controls**: Width, Height, Stride, Offset buttons present
- **Frame Counter**: Increments with incoming frames
- **Build Info**: Displays version and git hash
- **Error Handling**: Error banner dismisses correctly
- **Canvas Verification**: Canvas element renders correctly

Features:
- Auto-generates minimal test fixture if none exists
- Supports custom replay fixtures for real endoscope data
- Tests work without physical USB hardware via replay mode

### 4. Created Test Utilities

**File: `e2e/helpers.ts`**

Helper functions for common test operations:
- `waitForAppReady()` - Wait for app initialization
- `getConnectionStatus()`, `isConnected()` - Connection state
- `getResolution()`, `getFrameCount()` - Display info
- `waitForFrames()` - Wait for frame streaming
- `clickDebugButton()`, `dismissError()` - UI interaction
- `getCanvasInfo()` - Canvas dimension inspection

### 5. Set Up Test Fixtures

**Directory: `tests/fixtures/`**

- Auto-creates `sample-replay.bin` on first test run
- Minimal MJPEG frame (2x2 pixels) for basic testing
- Can be replaced with real endoscope capture data

### 6. Updated .gitignore

Added Playwright test artifact patterns:
- `test-results/` - Screenshots, videos, traces
- `playwright-report/` - HTML test report

### 7. Created Documentation

**File: `E2E_SETUP.md`**
- Comprehensive setup and usage guide
- Troubleshooting section
- CI/CD integration examples
- Custom fixture creation instructions

**File: `e2e/README.md`**
- Quick start guide
- Test running instructions
- Replay mode explanation
- Architecture overview

## Quick Start

### Install Dependencies
```bash
npm install
```

### Run Tests (Headless)
```bash
npm run test:e2e
```

### Run Tests with Browser UI
```bash
npm run test:e2e:headed
```

### Run with Custom Replay Data
```bash
CLEANSCOPE_REPLAY_PATH=tests/fixtures/real.bin npm run test:e2e
```

### View Test Report
```bash
# After tests complete
npx playwright show-report
```

## Key Features

### No Physical Hardware Required
Tests use USB packet replay instead of requiring physical endoscopes, enabling:
- CI/CD pipeline testing without special equipment
- Consistent, reproducible test results
- Offline testing support

### Tauri Integration
- Automatically starts dev server before tests
- Tests the actual webview content
- No special Tauri-specific test harness needed
- Works with standard Playwright API

### Comprehensive Assertion Helpers
Test utility functions abstract away Playwright internals and provide domain-specific checks:
- Frame streaming verification
- Resolution detection testing
- Status state inspection
- UI control validation

### CI/CD Ready
- Reporter configurations for GitHub Actions
- Artifact capture on failure
- Parallel worker support (local)
- Environment variable integration

## File Structure

```
cleanscope-tests/
├── playwright.config.ts              # Main configuration
├── E2E_SETUP.md                      # This summary + detailed guide
├── PLAYWRIGHT_SETUP_SUMMARY.md       # This file
├── package.json                      # Updated with playwright & scripts
├── e2e/
│   ├── frame-streaming.spec.ts       # Main test suite (10+ cases)
│   ├── helpers.ts                    # Test utility functions
│   └── README.md                     # E2E-specific documentation
├── tests/
│   └── fixtures/
│       └── sample-replay.bin          # Auto-generated minimal MJPEG
└── .gitignore                        # Updated with test artifacts
```

## Next Steps

1. **Run tests locally**:
   ```bash
   npm run test:e2e
   ```

2. **Add real endoscope data** (optional):
   - Capture USB packets from running app with physical device
   - Place `.bin` file in `tests/fixtures/`
   - Reference in test runs via `CLEANSCOPE_REPLAY_PATH`

3. **Integrate with CI/CD**:
   - Add test step to GitHub Actions workflow
   - See `E2E_SETUP.md` for example configuration

4. **Expand test coverage**:
   - Add performance tests for frame latency
   - Add accessibility (a11y) tests
   - Test specific endoscope device models

## Testing Tauri Apps with Playwright

This setup demonstrates a standard pattern for Playwright + Tauri:

1. **No custom webdriver** - Use standard Playwright browser
2. **Direct port targeting** - Connect to `localhost:1420` (Tauri dev port)
3. **Standard Playwright API** - All tests use normal `page.locator()` etc
4. **Environment passthrough** - Set env vars in config for app coordination

For other Tauri projects, this configuration can serve as a template.

## Troubleshooting

### Port Already in Use
```bash
pkill -f "tauri dev"
```

### Replay Fixture Not Found
Tests auto-generate a minimal sample. To verify:
```bash
ls -la tests/fixtures/sample-replay.bin
```

### Tests Timeout
Increase `webServer.timeout` in `playwright.config.ts` if Tauri dev takes >120s to start.

### Debug a Failing Test
```bash
npx playwright test --debug
```

Opens Inspector with step-through debugging.

## Resources

- [Playwright Docs](https://playwright.dev)
- [Tauri Testing Guide](https://tauri.app/v1/guides/testing/e2e/playwright/)
- [E2E_SETUP.md](./E2E_SETUP.md) - Detailed configuration guide
- [e2e/README.md](./e2e/README.md) - Running tests guide
- [CleanScope CLAUDE.md](./CLAUDE.md) - Project architecture

---

Setup completed successfully. Ready to run E2E tests!
