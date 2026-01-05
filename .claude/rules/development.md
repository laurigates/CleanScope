# Development Workflow

## TDD Workflow

Follow strict RED -> GREEN -> REFACTOR cycle:
1. Write a failing test that defines desired behavior
2. Implement minimal code to make the test pass
3. Refactor while keeping tests green

## Commit Conventions

Use conventional commits:
- `feat:` - New features
- `fix:` - Bug fixes
- `refactor:` - Code changes that neither fix bugs nor add features
- `docs:` - Documentation changes
- `test:` - Adding or updating tests
- `chore:` - Build process, tooling changes

## Platform-Specific Code

Use `#[cfg(target_os = "android")]` for Android-only code paths:
- JNI calls
- USB device handling
- android_logger initialization
- libusb_android module (entire module is Android-only)

Desktop builds should compile cleanly with stubbed functionality.

## Tauri Commands

When adding new Tauri commands:
1. Define the command function with `#[tauri::command]`
2. Register in `invoke_handler` macro in `lib.rs`
3. Call from frontend via `invoke("command_name", { args })`

## State Management

**AppState in lib.rs:**
- `frame_buffer` - Latest frame data (RGB or JPEG)
- `display_settings` - Width, height, stride, row_offset overrides
- `streaming_config` - Skip MJPEG toggle, YUV format selection
- `width_index`, `stride_index` - Index into `WIDTH_OPTIONS`, `STRIDE_OPTIONS`

**Important:** Settings indexes (stride_index, width_index) are separate from DisplaySettings.
Use them correctly when computing actual values in streaming code.

## Key Code Patterns

**Frame buffer updates:** Always set `width` and `height` when storing frames:
```rust
buffer.frame = rgb_data;
buffer.width = width;
buffer.height = height;
```

**Stride calculation:** Trust actual frame data over UVC descriptors:
```rust
let actual_stride = (frame_size as u32) / height;
let actual_width = actual_stride / 2;  // YUY2 = 2 bytes/pixel
```

**UVC header validation:** Use relaxed validation for cheap cameras:
- Require EOH bit (0x80) in byte 1
- Accept length 2-12 even if flags don't match
