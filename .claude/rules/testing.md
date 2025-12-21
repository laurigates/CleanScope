# Testing Requirements

## Rust Backend

Run checks before committing:
```bash
cargo check
cargo clippy
cargo test
```

## Frontend

Type-check Svelte/TypeScript:
```bash
npm run check
```

## Android Testing

Test on physical device (emulators lack USB OTG support):
1. Enable USB Debugging on device
2. Connect device via ADB
3. Run `npm run tauri:android:dev`
4. Plug in USB endoscope
5. Monitor logs: `adb logcat -s CleanScope:*`

## Coverage Expectations

- Tauri commands: Unit tests for business logic
- USB handling: Integration tests where possible, manual device testing required
- Frontend: Component tests for UI state transitions
