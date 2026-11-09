# App icon

`ado-planner.svg` is the source. Everything else in this folder is generated from it. Don't edit those files by hand.

After changing the SVG, run:

```sh
python3 .github/scripts/icons.py generate
```

This needs `rsvg-convert` (`apt install librsvg2-bin` or `brew install librsvg`). Each size is rendered from the SVG, not scaled down from a large PNG. Commit the SVG, the outputs, and `manifest.json` together.

| File               | Sizes                                       | Used by                                                                                 |
| ------------------ | ------------------------------------------- | --------------------------------------------------------------------------------------- |
| `png/NxN.png`      | 16, 24, 32, 48, 64, 128, 256, 512           | Linux hicolor icons in the AppImage; 128 is the X11 window icon; 512 is the README logo |
| `ado-planner.svg`  | scalable                                    | AppImage `hicolor/scalable`                                                             |
| `ado-planner.ico`  | 16, 20, 24, 32, 40, 48, 64 (BMP); 256 (PNG) | exe resource (`crates/app/build.rs`), NSIS installer, Start menu and desktop shortcuts  |
| `ado-planner.icns` | 16, 32 (ARGB); 32 to 1024 (PNG)             | macOS bundles                                                                           |

CI runs `python3 .github/scripts/icons.py check`. It fails if the SVG has rasters or external references, if an output is missing or the wrong size, or if `manifest.json` doesn't match the SVG and outputs. The release workflow also checks the built exe, installer, and AppImage.
