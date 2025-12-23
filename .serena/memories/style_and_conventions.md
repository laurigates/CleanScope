# CleanScope - Code Style and Conventions

## TypeScript/Svelte (Frontend)

**Formatter**: Biome 2.x (`biome.json`)
- Indent: 2 spaces
- Line width: 100
- Quotes: double
- Semicolons: always
- Trailing commas: all

**Svelte 5 Patterns**:
- Use runes syntax: `$state()`, `$derived()`, `$effect()`
- Reactive variables with `let x = $state<Type>(initial)`
- Event handlers: `onclick={handler}` (not `on:click`)

**Imports**:
- Organize imports (Biome auto-sorts)
- Tauri API: `import { invoke } from "@tauri-apps/api/core"`
- Events: `import { listen } from "@tauri-apps/api/event"`

## Rust (Backend)

**Formatter**: rustfmt (`src-tauri/rustfmt.toml`)
- Edition: 2021
- Max width: 100
- Indent: 4 spaces
- Import grouping: std, external, crate

**Lints**: Clippy (`Cargo.toml` [lints.clippy])
- `all = warn`
- Allowed for early dev: `dead-code`, `needless-pass-by-value`, `unnecessary-wraps`

**Patterns**:
- Platform-specific: `#[cfg(target_os = "android")]`
- Tauri commands: `#[tauri::command]` attribute
- Error handling: `Result<T, String>` for Tauri commands
- Logging: `log::info!()`, `log::debug!()`, `log::error!()`

## Naming Conventions

| Context | Convention | Example |
|---------|------------|---------|
| Rust functions | snake_case | `check_usb_status` |
| Rust structs | PascalCase | `UsbStatus` |
| TypeScript functions | camelCase | `cycleResolution` |
| TypeScript types | PascalCase | `Resolution` |
| Svelte components | PascalCase | `App.svelte` |
| CSS classes | kebab-case | `status-bar` |

## Commit Messages

Conventional commits enforced by pre-commit hook:
- `feat:` new features
- `fix:` bug fixes
- `chore:` tooling, config
- `docs:` documentation
- `refactor:` code changes without behavior change
