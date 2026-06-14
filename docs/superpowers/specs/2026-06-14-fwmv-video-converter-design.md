# FWMV Video Converter — Design Spec

**Date:** 2026-06-14
**Status:** Approved for planning

## Summary

A self-contained desktop app that converts ordinary videos (MP4, MKV, MOV, …)
into `.fwmv` files for the FreeWili movie player. It runs on Windows, Linux, and
macOS as a **single executable with no runtime dependencies** — no Python, no
separate `ffmpeg` install, no shared libraries. FFmpeg's decoding/scaling/encoding
libraries are statically linked into the binary; the only thing the user installs
is the one file.

This replaces the existing `tools/convert.ps1` + Python `pack_fwmv` workflow (which
requires an `ffmpeg` install, a Python install, and running from the movieplayer
repo root) with a portable double-click app.

## Goals

- Convert one or many videos in a batch to `.fwmv`.
- Pick multiple input files via a native file dialog.
- Pick a destination folder via a native folder dialog.
- Single self-contained binary per OS; nothing else to install.
- Output is byte-compatible with the FreeWili player (the FWMV v2 format).

## Non-Goals (v1)

- The flash / `.uf2` delivery path and the 15 MB trim budget. v1 produces
  **thumb-drive `.fwmv` only** (full length, `max_bytes = 0`, no trimming).
- User-configurable conversion settings. v1 uses fixed defaults: **480×270,
  15 fps, JPEG quality 10, 16 kHz mono audio.**
- Trimming controls (start/duration), resolution changes, or alternate codecs.
- Streaming-to-disk packing (v1 buffers per-file in memory; see Risks).

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | One toolchain for all 3 OSes; mature FFmpeg bindings; `rfd` native dialogs; clean static linking. |
| Video decode/scale/encode | Statically linked FFmpeg libs | Decoding modern codecs from scratch is infeasible; static link keeps it a single self-contained binary. |
| Pipeline | libavfilter filtergraph (Approach A) | Reproduces the documented `ffmpeg` filter chain byte-faithfully; minimal hand-rolled logic. |
| UI | `eframe`/`egui` + `rfd` | Lightweight immediate-mode GUI; native dialogs; no runtime deps. |
| Output scope | Thumb-drive `.fwmv` only | General-purpose "convert my videos" case; keeps the app focused. |
| Settings | Fixed defaults, no knobs | Simplest UI; defaults are the player's panel size and known-good values. |
| Filename collisions | Auto-suffix `_1`, `_2`, … | A batch never silently clobbers existing files; no prompts mid-batch. |
| FFmpeg build license | LGPL static build | We only decode inputs + encode MJPEG (built-in); no GPL components needed. License-clean. |

## Architecture

Three layers, each independently understandable and testable:

### 1. UI layer (`eframe`/`egui` + `rfd`)

A single window:

- **Add files…** button → `rfd` native multi-select file dialog. Appends to a
  queue list (dedup by absolute path).
- **Choose destination…** button → `rfd` native folder picker. Shows the chosen
  path.
- **Queue list:** each row shows the file name and a status
  (`queued` / `converting…` / `done` / `failed: <reason>`).
- **Convert** button: disabled until there is ≥1 file and a destination.
  Spawns a background worker thread.
- **Progress:** overall progress bar (files completed / total) plus a scrolling
  log line for the current file.

The UI thread never blocks. The worker thread sends progress events
(`FileStarted`, `FileProgress{frac}`, `FileDone`, `FileFailed{err}`,
`BatchDone`) over an `std::sync::mpsc` channel; the UI drains the channel each
frame and repaints.

### 2. Converter core

Pure orchestration over the FFmpeg bindings (`ffmpeg-next` / `ffmpeg-sys-next`).
Per input file:

1. **Demux** with `libavformat`; locate the best video stream and (optionally)
   the best audio stream.
2. **Video path:** decode frames (`libavcodec`) → push through a `libavfilter`
   graph equivalent to:
   `scale=480:270:force_original_aspect_ratio=decrease,pad=480:270:(ow-iw)/2:(oh-ih)/2,fps=15`
   → encode each filtered frame with the `libavcodec` **mjpeg** encoder at
   `qscale = 10` (matching `ffmpeg -q:v 10`). Collect `Vec<Vec<u8>>` of complete
   JPEG frames.
3. **Audio path (if present):** decode → `libavfilter`/`swresample` graph to
   **mono, 16 kHz, signed-16-bit** samples. Collect `Vec<i16>`.
4. Hand frames + PCM to the FWMV packer.

Output: each `input.<ext>` → `<dest>/input.fwmv` (collision → `input_1.fwmv`, …).

**Fidelity notes:**
- The mjpeg encoder produces `yuvj420p` baseline JPEGs, matching the CLI.
- Frame count, fps (15/1), dimensions (480×270), and audio rate (16000) are
  passed through to the packer exactly as the Python pipeline does.

### 3. FWMV packer (pure Rust port)

A faithful, dependency-free port of the authoritative Python tools:

- `fwmv_format.py` → header layout (`FWMV`, version 2, 64-byte header), the
  8-byte record header, record padding to 4-byte alignment, and the `FWIX` seek
  index (`<IIIhBB>` 16-byte entries, ≤600 entries).
- `pack_fwmv.py` → the packing algorithm: 1-second audio lead-in, interleaved
  video/audio record stream, per-frame ADPCM byte chunking, index-entry frame
  selection (interval grows so the table never exceeds 600), and the END record.
  Called with `max_bytes = 0` (no trim path needed in v1).
- `adpcm.py` → the IMA-ADPCM (mono, 4-bit, low-nibble-first, predictor 0 / step
  index 0) encoder and the `scan_states` pass that records decoder state at each
  index entry for glitch-free seeking.

This layer is pure integer logic, holds no FFmpeg types, and is unit-tested in
isolation.

## Data Flow

```
input.mp4 ─ libavformat demux ─┬─ video → libavcodec decode → libavfilter
                               │          (scale + pad + fps=15)
                               │        → libavcodec mjpeg encode (q=10)
                               │        → frames: Vec<Vec<u8>>
                               └─ audio → libavcodec decode → resample
                                          (mono, 16 kHz, s16) → Vec<i16>
   frames + pcm
        → fwmv::pack(width=480, height=270, fps=15/1, codec=MJPEG,
                     audio_rate=16000, max_bytes=0)
        → write <dest>/input.fwmv
```

## Error Handling

- **Per-file isolation:** failures (unsupported/corrupt file, decode error,
  unwritable destination) mark that row `failed: <reason>` and the batch
  continues with the next file.
- **No audio track:** produce a video-only `.fwmv` with `FLAG_AUDIO` unset
  (the format already supports the no-audio path).
- **Empty/zero-frame input:** fail that file with a clear message rather than
  writing an invalid file.
- **Destination not writable:** surface the OS error on the affected file(s).

## Testing Strategy

- **Golden-vector unit tests (format correctness):** run the existing Python
  tools (`adpcm.encode`, `pack` with known frames/PCM) to generate reference
  bytes, then assert the Rust packer and ADPCM encoder produce **byte-for-byte
  identical** output. This pins format correctness independent of FFmpeg version.
- **ADPCM round-trip:** encode → decode with the Rust port and compare against
  the Python decoder on random PCM.
- **Integration test:** convert a tiny bundled sample clip; parse the result
  back with the repo's Python `parse_header` / `iter_records` / `parse_index`;
  assert header fields, frame count, audio flags, and a well-formed index;
  confirm the reference C decoder (`src/video/fwmv.c`) accepts it.
- **CI:** build the static single-file binary for Windows, Linux, and macOS.

## Build & Dependency Reality (primary engineering cost)

The packer and UI are small. The bulk of the effort is **producing a static
FFmpeg build for three OSes** and linking it.

- **Components needed:** `libavformat`, `libavcodec`, `libavutil`, `libswscale`,
  `libswresample`, `libavfilter`. Enable common input decoders
  (H.264, HEVC, VP9, AV1, AAC, MP3, …) and the built-in **mjpeg** encoder.
- **License:** an **LGPL** configuration (no `--enable-gpl`, no x264/x265).
  We only *decode* inputs and *encode MJPEG*, so no GPL encoder is required —
  the result is license-clean for static distribution.
- **Linking:** `ffmpeg-sys-next` with `FFMPEG_DIR` pointing at the static build
  (vendored prebuilt static libs per platform, or built in CI).
- **Binary size:** ≈ 20–40 MB. Acceptable for a "download one file" tool.

## Risks & Open Items

- **Static FFmpeg build is the schedule driver.** Cross-platform static builds
  (especially macOS universal and Windows MSVC vs MinGW) need care. Mitigation:
  start with one platform end-to-end, then port the build recipe.
- **Memory use:** v1 buffers all JPEG frames + full audio per file in memory
  (~135 MB for a 10-min 15 fps clip). Acceptable for v1; streaming-pack is a
  noted future optimization (no trim path means records can be emitted as they
  are produced, with index/ADPCM state computed on the fly).
- **JPEG byte-identity vs the CLI** is not guaranteed across FFmpeg versions;
  tests assert structural/format compatibility and player acceptance, not
  byte-identical JPEG payloads.

## Definition of "Zero Dependencies"

The shipped artifact is **one executable per OS with no runtime dependencies**:
no Python, no `ffmpeg` install, no shared libraries to deploy. FFmpeg's
libraries are compiled in statically. The *build* uses FFmpeg source/libs and
Rust crates; the *user* installs nothing but the binary.
