# CleanScope - Task Completion Checklist

## Before Committing

Run these checks after completing any task:

### 1. Frontend Checks
```bash
npm run check          # TypeScript/Svelte type checking
npm run lint           # Biome linting
npm run format:check   # Formatting verification
```

### 2. Rust Checks
```bash
cd src-tauri
cargo check            # Compilation check
cargo clippy           # Linting
cargo fmt --check      # Formatting verification
```

### 3. Build Verification
```bash
npm run build          # Frontend build
npm run tauri:dev      # Desktop app runs
```

### 4. Pre-commit Hooks
```bash
pre-commit run --all-files
```

This runs:
- trailing-whitespace
- end-of-file-fixer
- check-yaml, check-json, check-toml
- check-merge-conflict
- detect-secrets
- biome-check
- cargo-fmt
- cargo-clippy

## When Adding New Features

1. **Tauri Command**: Add to `lib.rs`, register in `invoke_handler`
2. **Frontend Call**: Use `invoke("command_name", { args })`
3. **Android-specific**: Wrap in `#[cfg(target_os = "android")]`
4. **USB Device**: Update `device_filter.xml` if new vendor/product IDs

## Commit Message Format

```
type: short description

- Detail 1
- Detail 2

🤖 Generated with [Claude Code](https://claude.ai/code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
```

Types: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`
