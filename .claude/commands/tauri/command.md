Add a new Tauri command.

Arguments:
- $ARGUMENTS: command_name [param:type ...]

Example: `get_usb_devices` or `set_resolution width:u32 height:u32`

Steps:
1. Parse command name and parameters
2. Add to src-tauri/src/lib.rs:
   - #[tauri::command] function
   - Return type Result<T, String>
3. Register in invoke_handler macro
4. Generate frontend example:
   ```typescript
   const result = await invoke("command_name", { param: value });
   ```
5. Run `cargo check` to verify

Command Template:
```rust
#[tauri::command]
fn <command_name>(<params>) -> Result<(), String> {
    // Implementation
    Ok(())
}
```

Registration:
```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands
    <command_name>,
])
```

Frontend Usage:
```typescript
import { invoke } from "@tauri-apps/api/core";

const result = await invoke("<command_name>", { param: value });
```

Output:
- Modified lib.rs with new command
- TypeScript invoke example
- cargo check verification result
