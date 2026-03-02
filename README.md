# H264BSAnalyzer

H.264/AVC and H.265/HEVC bitstream analyzer (Rust, cross-platform GUI).

**License:** [MIT](LICENSE)

## Features

- **NAL list:** VPS, SPS, PPS, SEI, AUD, Slice with type-based colors, column alignment, selection.
- **Hex view:** Raw hex and ASCII for the selected NAL.
- **File information:** Resolution, cropping, profile/level, encoding (CABAC/CAVLC), frame count.
- **NAL parsing details:** Foldable sections (NAL Unit, SPS, PPS, slice_type, etc.).
- **Theme:** Dark / Light; title bar follows theme; settings persisted.
- **Recent files:** File Recent (last 10), stored in config dir (`h264bsanalyzer/config.json`).
- **Playback (optional):** Play / Stop / Next frame when built with `decode` or `decode-vcpkg`.
- **Export:** Save current frame as BMP/JPEG/YUV/RGB (when decode is enabled).

## Code structure

| Module | Description |
|--------|-------------|
| `model` | `FileType`, `NaluInfo`, `ParseResult`, `SpsInfo`/`PpsInfo`, NAL/slice types |
| `parser` | Annex B scan, H.264/H.265 parse, `parse_file`, `parse_nal_detail`, `tree_text_for_nal` |
| `decode` | Optional FFmpeg decode (rsmpeg), frame-by-frame playback |
| `export` | BMP, JPEG, YUV, RGB frame export |
| `gui` | egui UI: NAL list, hex view, file/parse info panels, theme, recent files |

## Build and run

**Prerequisites:** [Rust](https://rustup.rs/) (stable). No FFmpeg needed for NAL parsing and export.

```bash
cargo build --release
cargo run --release
```

Output: `target/release/h264bsanalyzer` (or `.exe` on Windows).

**Optional playback (decode):** Requires FFmpeg and a linking method.

- **Windows (vcpkg):**
  ```bash
  vcpkg install ffmpeg:x64-windows
  cargo build --release --features decode-vcpkg
  ```
- **Windows (prebuilt):** Set `FFMPEG_LIBS_DIR` (and `FFMPEG_INCLUDE_DIR` if needed), then `cargo build --release --features decode`.
- **Linux / macOS:** Install FFmpeg dev packages (e.g. `libavcodec-dev` / `ffmpeg`), then `cargo build --release --features decode` (set `FFMPEG_PKG_CONFIG_PATH` if needed).

**Platform notes:**
- **Windows:** x86_64; GUI uses software rendering (glow).
- **Linux:** x86_64; install `libxcb`, `libxcb-render`, `libxcb-shape`, `libxcb-xfixes` and similar.
- **macOS:** x86_64 or arm64 (Apple Silicon); no extra deps for default build.

## Usage

Use **File Open** or drag a file into the window. Supported suffixes: `.h264`, `.264`, `.avc`, `.h265`, `.265`, `.hevc`, or format is detected from content. Click a NAL in the list to see details and hex; use **Play** when decode is enabled.

## License

[MIT License](LICENSE)
