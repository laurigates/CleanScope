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

Desktop builds should compile cleanly with stubbed functionality.

## Tauri Commands

When adding new Tauri commands:
1. Define the command function with `#[tauri::command]`
2. Register in `invoke_handler` macro in `lib.rs`
3. Call from frontend via `invoke("command_name", { args })`
