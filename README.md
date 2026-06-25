# AIAS Electron

AIAS is a Windows desktop texture toolbox for PBR channel packing, DDS conversion, mipmap generation, and War Thunder UserSkins management.

This repository now contains an Electron/JavaScript rebuild beside the original Python implementation. The Python entry point remains in `AIAS.py` for reference while the active Electron app starts from `src/main/main.js`.

## Requirements

- Node.js 20 or newer
- Windows, for `tools/texconv.exe`

## Install

```powershell
npm install
```

## Run

```powershell
npm start
```

## Check

```powershell
npm run check
```

## Build

```powershell
npm run build
```

## Project Layout

- `src/main/main.js` - Electron main process and IPC wiring.
- `src/main/preload.js` - safe renderer bridge.
- `src/main/services/texture-service.js` - PBR packing, DDS conversion, mipmap assembly, and splitting.
- `src/main/services/skin-service.js` - War Thunder UserSkins detection and file operations.
- `src/main/services/settings-service.js` - JSON settings stored in Electron user data.
- `src/renderer/index.html` - application UI.
- `src/renderer/scripts/app.js` - renderer state and actions.
- `src/renderer/styles/app.css` - application styling.

## Notes

DDS encoding and decoding intentionally use the bundled DirectXTex `texconv.exe`. JavaScript handles orchestration and channel packing, while `texconv` handles the format-specific DDS work.
