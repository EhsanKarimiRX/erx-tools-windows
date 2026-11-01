# Adding a tool

The exact recipe for adding a new tool to ERX Tools for Windows. Every step
references real code — keep this file in sync when the structure changes.

This example adds a tool with id `my-tool` (kebab-case). Replace the name
everywhere.

## 1. Frontend

### 1.1 Create the tool folder

Copy the template and adjust it:

```
src/tools/my-tool/index.tsx
```

```tsx
/** My Tool — short description of what it does. */
export default function MyTool() {
  return <p className="tool-placeholder">This tool is not built yet.</p>;
}
```

Rules:

- The default export is the tool's root component. It renders **content
  only** — the header (Back button + title) comes from `ToolPage` via
  `ToolLayout`.
- Never import from another tool. Shared UI goes to `src/shared/`.
- Do not add a route or navigation code — the registry drives everything.

### 1.2 Register the tool

Open `src/tools/registry.ts` and add one entry to the `tools` array:

```ts
{
  id: "my-tool",                 // kebab-case, stable, unique
  name: "My Tool",               // shown on the tile and in the tool header
  description: "One line",       // tile tooltip
  icon: SomeIcon,                // a lucide-react icon component
  load: () => import("./my-tool"), // lazy loader — creates a per-tool chunk
  status: "ready",               // "ready" opens; "coming-soon" dims the tile
},
```

Import the icon at the top of `registry.ts` from `lucide-react`
(https://lucide.dev/icons for available names). That's it — the home screen,
tooltip, lazy loading, error boundary and navigation all come for free.

### 1.3 Check

`npm run tauri dev` → the tile appears; click opens the tool; Back returns.
In devtools you should see a separate chunk fetched when the tool opens
(`src/tools/my-tool/index.tsx`).

## 2. Rust backend (only if the tool needs commands)

Tools do real work (files, images, encryption) in Rust. The UI never touches
the file system directly.

### 2.1 Create the module

Create `src-tauri/src/tools/my_tool.rs` (snake_case of the id).
For a bigger tool, make it a folder instead and keep pure logic separate
from the Tauri layer, e.g. `image_convert/mod.rs` + `core.rs` (unit-testable,
no Tauri types) + `commands.rs` (`#[tauri::command]`s only).

```rust
use serde::Serialize;

#[derive(Serialize)]
pub struct MyResult {
    pub message: String,
}

/// One-line doc comment per command.
#[tauri::command]
pub fn my_command(input: String) -> Result<MyResult, crate::tools::ToolError> {
    if input.is_empty() {
        // User-friendly English message; log internal details first if any.
        return Err(crate::tools::ToolError::Internal(
            "Please enter a value first.".into(),
        ));
    }
    Ok(MyResult { message: format!("got: {input}") })
}
```

Register the module in `src-tauri/src/tools/mod.rs`:

```rust
pub mod my_tool;
```

### 2.2 Register the command (the ONE central place)

In `src-tauri/src/lib.rs`, add one line to the `generate_handler!` list:

```rust
tools::my_tool::my_command,
```

### 2.3 Call it from the tool component

```tsx
import { invoke } from "@tauri-apps/api/core";

const result = await invoke<{ message: string }>("my_command", {
  input: "hello", // argument names are camelCase in JS, snake_case in Rust
});
```

### 2.4 Errors

Return `Result<T, crate::tools::ToolError>` from commands that can fail.
Add a variant to `ToolError` in `src-tauri/src/tools/error.rs` only if the
existing ones don't fit; its `Display` string is what the user sees, so keep
it a friendly English sentence. When the first real error variant is used,
remove the `#[allow(dead_code)]` attribute there.

### 2.5 Permissions

If the tool needs a Tauri plugin (dialog, fs, …):

1. Add the plugin to `src-tauri/Cargo.toml` and its JS package to
   `package.json` only if no other tool already brought it in.
2. Initialize it in `lib.rs` (`.plugin(tauri_plugin_x::init())`).
3. Grant the **narrowest** permission in the capability file
   `src-tauri/capabilities/default.json`, and write a one-line comment (in
   the capability description or DECISIONS.md) saying which tool needs it and
   why. Never use `**` broad permissions when a scoped one exists.

## 3. Quality gate (always)

```powershell
npm run build                 # strict TS + production build
cd src-tauri
cargo fmt                     # format
cargo clippy -- -D warnings   # zero warnings
```

Then test in the running app: tile looks right, tool opens lazily, Back
works, and a deliberate error inside the tool shows the error screen without
killing the app.

## 4. Checklist

- [ ] `src/tools/my-tool/index.tsx` created (content only, default export)
- [ ] Entry added to `src/tools/registry.ts` (id, name, description, icon,
      lazy `load`, status)
- [ ] (optional) `src-tauri/src/tools/my_tool.rs` with `#[tauri::command]`s
- [ ] Module added to `src-tauri/src/tools/mod.rs`
- [ ] Commands added to `generate_handler!` in `src-tauri/src/lib.rs`
- [ ] (optional) Narrow permissions added to capabilities, documented
- [ ] `npm run build`, `cargo fmt`, `cargo clippy -- -D warnings` pass
- [ ] Manual test in `npm run tauri dev`
