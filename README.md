# AIAS Tauri

AIAS is a Windows desktop texture toolbox for PBR channel packing, DDS conversion, mipmap generation, and War Thunder UserSkins management.

This version uses Tauri 2.x for the desktop shell and a native HTML/CSS/JavaScript renderer. Legacy desktop shells and Python GUI entry points are no longer part of the active desktop app.

## Requirements

- Windows
- Node.js 20 or newer
- npm
- Visual Studio Code
- Rust toolchain for Tauri 2.x (`rustc` and `cargo`)
- Microsoft Visual Studio Build Tools with the Desktop development with C++ workload
- Microsoft Edge WebView2 Runtime

## Install

```powershell
npm install
```

## Run With Hot Reload

```powershell
npm run tauri dev
```

This starts Vite at `http://127.0.0.1:5173/` and opens the Tauri desktop window against that live renderer. Changes under `src/renderer` update like a normal web app.

## Renderer-Only Preview

```powershell
npm run dev:renderer
```

Use this when editing UI layout only. Local filesystem operations are available in the Tauri window, not in a plain browser preview.

## Check

```powershell
npm run check
```

## Build Installer

```powershell
npm run build
```

The Windows installer is produced by Tauri under `src-tauri/target/release/bundle/`.

## VS Code

Open this folder in Visual Studio Code.

- Run task `tauri: dev` for the full desktop app.
- Run task `renderer: dev` for web-only UI preview.
- Use launch configuration `AIAS Tauri Dev` to start the app from the Run and Debug panel.

## Project Layout

```text
AIAS/
  src/
    renderer/
      index.html
      scripts/app.js
      styles/app.css
      styles/performance.css
  src-tauri/
    Cargo.toml
    tauri.conf.json
    build.rs
    capabilities/default.json
    src/main.rs
  package.json
  vite.config.js
```

## Notes

- UI is native HTML/CSS/JavaScript. No TypeScript and no heavy frontend framework.
- Desktop APIs use Tauri commands and official Tauri plugins.
- DDS encoding, decoding, and mipmap generation run inside the Rust application. No external texture tools are required at runtime.
