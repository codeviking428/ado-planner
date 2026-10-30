# Rewrite the app in Rust with GPUI

We are replacing the Electron + React app with a native Rust app built on GPUI and gpui-kit, with Tokio for I/O. We want faster startup, lower memory, a single native binary, and a Gantt that stays smooth with thousands of rows, and we want a Rust codebase. The Rust app grows in `crates/` beside `src/`; Electron keeps shipping until the Rust app reaches parity, then one PR deletes the TypeScript.

## Considered Options

- **Keep Electron and optimize.** Virtualizing rows would fix the Gantt, but not startup, memory or binary size, and it would not give us Rust.
- **Tauri.** Native shell, but the UI is still a webview, so the Gantt's cost stays the same.
- **Rewrite in place.** Nothing would ship until the Rust app was usable.

## Consequences

- gpui-component, with four Catppuccin themes, draws the chrome. The Gantt is hand-built on gpui-base, because no Rust Gantt fits the app.
- The Azure DevOps client is hand-written on `reqwest`, not the generated `azure_devops_rust_api`. It keeps compile times low, and a configurable base URL points it at the mock server.
- Everyone signs in again once at cutover. Electron's `safeStorage` files can't be read from Rust.
- HTML fields lose TipTap editing; see ADR-0002.
