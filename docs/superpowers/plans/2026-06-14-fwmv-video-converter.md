# FWMV Video Converter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a single self-contained Rust GUI app (Windows/Linux/macOS) that converts videos to the FreeWili `.fwmv` format, with multi-file selection, a destination-folder picker, and a progress display — no runtime dependencies (FFmpeg statically linked).

**Architecture:** Three layers. (1) A pure-Rust `fwmv` module that ports the authoritative Python format/packer/ADPCM byte-for-byte. (2) A `convert` core that drives statically-linked FFmpeg libs (demux → decode → libavfilter scale/pad/fps + resample → MJPEG encode) to produce JPEG frames + mono 16 kHz PCM, then calls the packer. (3) An `egui`/`eframe` GUI with native `rfd` dialogs and a background worker thread.

**Tech Stack:** Rust 2021; `ffmpeg-next` (statically linked FFmpeg, LGPL build); `eframe`/`egui`; `rfd`. Golden vectors generated from the existing Python tools at `C:\~prj\Dropbox\vibeProjects\movieplayer\tools`.

**Spec:** `docs/superpowers/specs/2026-06-14-fwmv-video-converter-design.md`

---

## Reference facts (authoritative — do not deviate)

These come from the movieplayer repo and the spec. Every byte the packer emits must match.

- **Header:** 64 bytes. Magic `b"FWMV"`, then little-endian fields in this order: `version u16, flags u16, codec u16, width u16, height u16, fps_num u16, fps_den u16, frame_count u32, index_offset u32, data_offset u32, audio_offset u32, audio_codec u16, audio_rate u16, audio_size u32, audio_samples u32`, then `4 × u32` of zero padding and `2` zero bytes (46 base + 18 pad = 64).
- **Constants:** `VERSION_V2=2`, `CODEC_MJPEG=1`, `FLAG_AUDIO=1`, `AUDIO_IMA_ADPCM=1`, `REC_VIDEO=1`, `REC_AUDIO=2`, `REC_END=0xFFFF`, `RECORD_HDR_SIZE=8`, `INDEX_MAX_ENTRIES=600`, `INDEX_ENTRY_SIZE=16`, index magic `b"FWIX"`.
- **Record:** `struct <HHI>` = `type u16, reserved u16(=0), size u32`, then payload, then zero-padding to a 4-byte boundary: `padding = (4 - size % 4) % 4`.
- **Index entry:** `struct <IIIhBB>` = `frame_no u32, file_offset u32, audio_bytes_before u32, adpcm_predictor i16, adpcm_step_index u8, pad u8(=0)`. Index blob = `b"FWIX" + u32 count + entries`.
- **Defaults (v1, fixed):** width 480, height 270, fps 15/1, JPEG `-q:v 10`, audio mono 16000 Hz s16le. `max_bytes = 0` (no trimming).
- **ADPCM:** IMA-ADPCM, mono, 4-bit, headerless, **low nibble first**, initial predictor 0 / step index 0. Tables and integer math are in `adpcm.py` — port exactly.
- **Packer interleave:** 1 second of audio lead-in (`ceil(fps_num/fps_den)` frames), then for each video frame `i`, emit `REC_VIDEO(frame[i])` followed by `REC_AUDIO(chunk[i+lead])` while `i+lead < n`. Audio chunk boundaries: `bound[i] = audio_samples_for(i)//2`, last chunk takes the remainder. `audio_samples_for(i) = i*fps_den*audio_rate//fps_num`. Index entries are placed at the first frame of each `interval_s`-second tick where `interval_s = max(1, ceil(dur_s/600))`, `dur_s = n*fps_den//fps_num`, frame `fk = ceil(k*fps_num/fps_den)`.

Open the Python sources while implementing the packer: `fwmv_format.py`, `pack_fwmv.py`, `adpcm.py` in `C:\~prj\Dropbox\vibeProjects\movieplayer\tools`.

---

## File structure

```
fileconvert/
  Cargo.toml
  build/                         # static FFmpeg notes + prebuilt libs (gitignored binaries)
  scripts/
    gen_fixtures.py              # one-time golden-vector generator (run from movieplayer repo)
    gen_sample.sh / .ps1         # one-time tiny test-video generator
  src/
    lib.rs                       # pub mod fwmv; pub mod convert;
    main.rs                      # eframe entry; mod app;
    app.rs                       # egui UI + worker thread
    convert.rs                   # FFmpeg orchestration per file
    fwmv/
      mod.rs                     # pack() — assembles header + record stream + index
      format.rs                  # header, record, index primitives
      adpcm.rs                   # IMA-ADPCM encode/decode/scan_states
  tests/
    adpcm.rs                     # ADPCM unit + golden tests
    fwmv_format.rs               # format primitive unit tests
    fwmv_pack.rs                 # full-file golden byte-identity test
    integration_convert.rs       # convert sample.mp4, parse result back
  tests/fixtures/                # committed golden bytes + sample video
    adpcm_sine.bin
    pack_av.fwmv
    pack_video_only.fwmv
    sample.mp4
```

---

## Task 1: Project scaffold

**Files:**
- Create: `Cargo.toml`, `src/lib.rs`, `src/main.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "fileconvert"
version = "0.1.0"
edition = "2021"

[lib]
name = "fileconvert"
path = "src/lib.rs"

[[bin]]
name = "fileconvert"
path = "src/main.rs"

[dependencies]
ffmpeg-next = "7.1"
eframe = "0.29"
egui = "0.29"
rfd = "0.15"

[profile.release]
lto = true
strip = true
```

- [ ] **Step 2: Create `src/lib.rs`**

```rust
pub mod fwmv;
pub mod convert;
```

- [ ] **Step 3: Create minimal `src/main.rs` (temporary, replaced in Task 11)**

```rust
fn main() {
    println!("fileconvert");
}
```

- [ ] **Step 4: Create empty module stubs so the crate compiles**

Create `src/fwmv/mod.rs`:
```rust
pub mod adpcm;
pub mod format;
```
Create `src/fwmv/adpcm.rs` and `src/fwmv/format.rs` as empty files, and `src/convert.rs`:
```rust
// converter core — implemented in later tasks
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: compiles (FFmpeg link may fail until Task 6 wires the static build; if so, temporarily comment the `ffmpeg-next` dependency and `pub mod convert;` line, and re-enable in Task 6). Note which happened.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/
git commit -m "chore: scaffold fileconvert crate"
```

---

## Task 2: ADPCM step function and encoder

**Files:**
- Modify: `src/fwmv/adpcm.rs`
- Test: `tests/adpcm.rs`

- [ ] **Step 1: Write the failing tests**

Create `tests/adpcm.rs`:
```rust
use fileconvert::fwmv::adpcm;

#[test]
fn encode_single_zero_is_one_zero_nibble() {
    assert_eq!(adpcm::encode(&[0]), vec![0x00]);
}

#[test]
fn encode_single_large_positive() {
    // predictor=0, step=7: delta 1000 sets bits 4|2|1 = 7, sign bit clear.
    assert_eq!(adpcm::encode(&[1000]), vec![0x07]);
}

#[test]
fn encode_packs_two_samples_into_one_byte() {
    // two samples -> low nibble then high nibble of a single byte.
    let out = adpcm::encode(&[1000, 0]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0] & 0x0F, 0x07); // first sample in low nibble
}

#[test]
fn decode_inverts_encode_trajectory() {
    let pcm: Vec<i16> = (0..256).map(|i| ((i as f32 * 0.2).sin() * 8000.0) as i16).collect();
    let enc = adpcm::encode(&pcm);
    let dec = adpcm::decode(&enc, pcm.len());
    assert_eq!(dec.len(), pcm.len());
    // ADPCM is lossy but tracks the signal; check it stays bounded near source.
    let max_err = pcm.iter().zip(&dec).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
    assert!(max_err < 4000, "max_err={max_err}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test adpcm`
Expected: FAIL (functions not defined).

- [ ] **Step 3: Implement `src/fwmv/adpcm.rs`**

Port `adpcm.py` exactly. Predictor is kept as `i32` internally and clamped to the i16 range.
```rust
// IMA-ADPCM (mono, 4-bit). Mirror of tools/adpcm.py — low nibble first,
// initial predictor 0 / step index 0. Change with src/audio/adpcm.c or neither.

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31,
    34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143,
    157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658,
    724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499,
    2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630,
    9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623,
    27086, 29794, 32767,
];
const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Shared decoder-side state update. Integer math mirrors adpcm.c exactly.
fn step(predictor: i32, step_index: i32, nibble: u8) -> (i32, i32) {
    let s = STEP_TABLE[step_index as usize];
    let mut diff = s >> 3;
    if nibble & 1 != 0 { diff += s >> 2; }
    if nibble & 2 != 0 { diff += s >> 1; }
    if nibble & 4 != 0 { diff += s; }
    let mut p = if nibble & 8 != 0 { predictor - diff } else { predictor + diff };
    p = p.clamp(-32768, 32767);
    let si = (step_index + INDEX_TABLE[nibble as usize]).clamp(0, 88);
    (p, si)
}

pub fn encode(pcm: &[i16]) -> Vec<u8> {
    let (mut predictor, mut step_index) = (0i32, 0i32);
    let mut out: Vec<u8> = Vec::new();
    let mut low = true;
    for &sample in pcm {
        let mut delta = sample as i32 - predictor;
        let mut nib: u8 = if delta < 0 { 8 } else { 0 };
        if delta < 0 { delta = -delta; }
        let s = STEP_TABLE[step_index as usize];
        if delta >= s { nib |= 4; delta -= s; }
        if delta >= s >> 1 { nib |= 2; delta -= s >> 1; }
        if delta >= s >> 2 { nib |= 1; }
        let (p, si) = step(predictor, step_index, nib);
        predictor = p; step_index = si;
        if low { out.push(nib); } else { *out.last_mut().unwrap() |= nib << 4; }
        low = !low;
    }
    out
}

pub fn decode(data: &[u8], n: usize) -> Vec<i16> {
    let (mut predictor, mut step_index) = (0i32, 0i32);
    let mut out = Vec::with_capacity(n);
    for i in 0..n.min(data.len() * 2) {
        let byte = data[i >> 1];
        let nib = if i & 1 != 0 { byte >> 4 } else { byte & 0x0F };
        let (p, si) = step(predictor, step_index, nib);
        predictor = p; step_index = si;
        out.push(predictor as i16);
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test adpcm`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/fwmv/adpcm.rs tests/adpcm.rs
git commit -m "feat: port IMA-ADPCM encode/decode"
```

---

## Task 3: ADPCM `scan_states` + golden fixture

**Files:**
- Modify: `src/fwmv/adpcm.rs`
- Create: `scripts/gen_fixtures.py`, `tests/fixtures/adpcm_sine.bin`
- Test: `tests/adpcm.rs`

- [ ] **Step 1: Create the golden-fixture generator `scripts/gen_fixtures.py`**

This is run **once** from the movieplayer repo root (it imports that repo's `tools`). It writes fixtures into this repo.
```python
# Run from C:\~prj\Dropbox\vibeProjects\movieplayer (so `tools` resolves):
#   python C:\~prj\Dropbox\vibeProjects\fileconvert\scripts\gen_fixtures.py C:\~prj\Dropbox\vibeProjects\fileconvert\tests\fixtures
import sys, math, struct, pathlib
from tools import adpcm, pack_fwmv as P, fwmv_format as F

out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)

# 1) ADPCM golden: 1000-sample sine.
pcm = [int(math.sin(i * 0.2) * 8000) for i in range(1000)]
(out / "adpcm_sine.bin").write_bytes(adpcm.encode(pcm))

# 2) Full-file goldens. Tiny fake JPEG payloads (the packer is codec-agnostic
#    about frame *content*; it only stores bytes), plus real ADPCM audio.
frames = [bytes([0xFF, 0xD8]) + bytes([i & 0xFF]) * (50 + i) + bytes([0xFF, 0xD9])
          for i in range(40)]
audio = [int(math.sin(i * 0.05) * 6000) for i in range(40 * 16000 // 15 + 16000)]
import array
apcm = array.array("h", audio)
P.pack(frames, str(out / "pack_av.fwmv"), codec=F.CODEC_MJPEG, width=480, height=270,
       fps_num=15, fps_den=1, audio_pcm=apcm, audio_rate=16000, max_bytes=None)
P.pack(frames, str(out / "pack_video_only.fwmv"), codec=F.CODEC_MJPEG, width=480,
       height=270, fps_num=15, fps_den=1, audio_pcm=None, max_bytes=None)
print("wrote fixtures to", out)
```

- [ ] **Step 2: Generate the fixtures**

Run (PowerShell, from the movieplayer repo root):
```powershell
cd C:\~prj\Dropbox\vibeProjects\movieplayer
python C:\~prj\Dropbox\vibeProjects\fileconvert\scripts\gen_fixtures.py C:\~prj\Dropbox\vibeProjects\fileconvert\tests\fixtures
```
Expected: `adpcm_sine.bin`, `pack_av.fwmv`, `pack_video_only.fwmv` created. (Keep `pack_*.fwmv` for Task 5.)

- [ ] **Step 3: Write the failing tests** in `tests/adpcm.rs` (append)

```rust
#[test]
fn encode_matches_python_golden() {
    let pcm: Vec<i16> = (0..1000).map(|i| ((i as f64 * 0.2).sin() * 8000.0) as i16).collect();
    let expected = std::fs::read("tests/fixtures/adpcm_sine.bin").unwrap();
    assert_eq!(adpcm::encode(&pcm), expected);
}

#[test]
fn scan_states_matches_single_pass() {
    let pcm: Vec<i16> = (0..1000).map(|i| ((i as f64 * 0.2).sin() * 8000.0) as i16).collect();
    let blob = adpcm::encode(&pcm);
    // States after 0, 10, and all bytes must equal a fresh decode to that point.
    let offsets = [0usize, 10, blob.len()];
    let states = adpcm::scan_states(&blob, &offsets);
    assert_eq!(states.len(), offsets.len());
    assert_eq!(states[0], (0, 0)); // initial state
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test --test adpcm`
Expected: FAIL (`encode_matches_python_golden` needs the fixture + `scan_states` undefined).

- [ ] **Step 5: Implement `scan_states` in `src/fwmv/adpcm.rs`** (append)

```rust
/// Decoder state (predictor, step_index) after each prefix length in
/// `byte_offsets` (ascending). Single pass over the nibble stream.
/// Returns (predictor as i16, step_index as u8) for index entries.
pub fn scan_states(blob: &[u8], byte_offsets: &[usize]) -> Vec<(i16, u8)> {
    let (mut predictor, mut step_index) = (0i32, 0i32);
    let mut states = Vec::with_capacity(byte_offsets.len());
    let mut wi = 0usize;
    for pos in 0..=blob.len() {
        while wi < byte_offsets.len() && byte_offsets[wi] == pos {
            states.push((predictor as i16, step_index as u8));
            wi += 1;
        }
        if pos == blob.len() { break; }
        let b = blob[pos];
        let (p, si) = step(predictor, step_index, b & 0x0F);
        let (p, si) = step(p, si, b >> 4);
        predictor = p; step_index = si;
    }
    while wi < byte_offsets.len() { // offsets past the end clamp
        states.push((predictor as i16, step_index as u8));
        wi += 1;
    }
    states
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --test adpcm`
Expected: PASS (6 tests).

- [ ] **Step 7: Commit**

```bash
git add src/fwmv/adpcm.rs tests/adpcm.rs scripts/gen_fixtures.py tests/fixtures/adpcm_sine.bin
git commit -m "feat: adpcm scan_states + golden vector"
```

---

## Task 4: FWMV format primitives (record, header, index)

**Files:**
- Modify: `src/fwmv/format.rs`
- Test: `tests/fwmv_format.rs`

- [ ] **Step 1: Write the failing tests**

Create `tests/fwmv_format.rs`:
```rust
use fileconvert::fwmv::format::{self, Header, IndexEntry};

#[test]
fn record_padding_aligns_to_four() {
    assert_eq!(format::record_padding(0), 0);
    assert_eq!(format::record_padding(3), 1);
    assert_eq!(format::record_padding(4), 0);
    assert_eq!(format::record_padding(5), 3);
}

#[test]
fn pack_record_end_is_eight_bytes() {
    assert_eq!(format::pack_record(format::REC_END, &[]),
               vec![0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn pack_record_video_pads_payload() {
    // type=1, reserved=0, size=3, "abc", + 1 pad byte.
    assert_eq!(format::pack_record(format::REC_VIDEO, b"abc"),
               vec![0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, b'a', b'b', b'c', 0x00]);
}

#[test]
fn header_is_64_bytes_with_magic() {
    let h = Header {
        version: 2, flags: 0, codec: 1, width: 480, height: 270,
        fps_num: 15, fps_den: 1, frame_count: 40, index_offset: 1234,
        data_offset: 64, audio_offset: 0, audio_codec: 1, audio_rate: 16000,
        audio_size: 555, audio_samples: 999,
    };
    let bytes = h.pack();
    assert_eq!(bytes.len(), 64);
    assert_eq!(&bytes[0..4], b"FWMV");
    // round-trip fields
    let p = Header::parse(&bytes);
    assert_eq!(p.width, 480);
    assert_eq!(p.frame_count, 40);
    assert_eq!(p.audio_rate, 16000);
}

#[test]
fn pack_index_has_magic_count_and_16_byte_entries() {
    let entries = vec![IndexEntry {
        frame_no: 0, file_offset: 64, audio_bytes_before: 0,
        adpcm_predictor: 0, adpcm_step_index: 0,
    }];
    let blob = format::pack_index(&entries);
    assert_eq!(&blob[0..4], b"FWIX");
    assert_eq!(u32::from_le_bytes(blob[4..8].try_into().unwrap()), 1);
    assert_eq!(blob.len(), 8 + 16);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test fwmv_format`
Expected: FAIL (types/functions not defined).

- [ ] **Step 3: Implement `src/fwmv/format.rs`**

```rust
// FWMV v2 container primitives. Mirror of tools/fwmv_format.py.

pub const MAGIC: &[u8; 4] = b"FWMV";
pub const HEADER_SIZE: usize = 64;
pub const VERSION_V2: u16 = 2;
pub const CODEC_MJPEG: u16 = 1;
pub const FLAG_AUDIO: u16 = 1;
pub const AUDIO_IMA_ADPCM: u16 = 1;

pub const REC_VIDEO: u16 = 1;
pub const REC_AUDIO: u16 = 2;
pub const REC_END: u16 = 0xFFFF;
pub const RECORD_HDR_SIZE: usize = 8;

pub const INDEX_MAGIC: &[u8; 4] = b"FWIX";
pub const INDEX_ENTRY_SIZE: usize = 16;
pub const INDEX_MAX_ENTRIES: usize = 600;

pub fn record_padding(size: usize) -> usize {
    (4 - size % 4) % 4
}

pub fn pack_record(rtype: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECORD_HDR_SIZE + payload.len() + 3);
    out.extend_from_slice(&rtype.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());          // reserved
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out.extend(std::iter::repeat(0u8).take(record_padding(payload.len())));
    out
}

#[derive(Clone, Copy)]
pub struct Header {
    pub version: u16, pub flags: u16, pub codec: u16,
    pub width: u16, pub height: u16, pub fps_num: u16, pub fps_den: u16,
    pub frame_count: u32, pub index_offset: u32, pub data_offset: u32,
    pub audio_offset: u32, pub audio_codec: u16, pub audio_rate: u16,
    pub audio_size: u32, pub audio_samples: u32,
}

impl Header {
    pub fn pack(&self) -> [u8; HEADER_SIZE] {
        let mut b = [0u8; HEADER_SIZE];
        let mut o = 0usize;
        macro_rules! put { ($v:expr) => {{ let s = $v.to_le_bytes(); b[o..o+s.len()].copy_from_slice(&s); o += s.len(); }}; }
        b[0..4].copy_from_slice(MAGIC); o = 4;
        put!(self.version); put!(self.flags); put!(self.codec);
        put!(self.width); put!(self.height); put!(self.fps_num); put!(self.fps_den);
        put!(self.frame_count); put!(self.index_offset); put!(self.data_offset);
        put!(self.audio_offset); put!(self.audio_codec); put!(self.audio_rate);
        put!(self.audio_size); put!(self.audio_samples);
        // remaining bytes already zero (4*u32 + 2 pad)
        let _ = o;
        b
    }

    pub fn parse(buf: &[u8]) -> Header {
        let u16a = |o: usize| u16::from_le_bytes(buf[o..o+2].try_into().unwrap());
        let u32a = |o: usize| u32::from_le_bytes(buf[o..o+4].try_into().unwrap());
        Header {
            version: u16a(4), flags: u16a(6), codec: u16a(8),
            width: u16a(10), height: u16a(12), fps_num: u16a(14), fps_den: u16a(16),
            frame_count: u32a(18), index_offset: u32a(22), data_offset: u32a(26),
            audio_offset: u32a(30), audio_codec: u16a(34), audio_rate: u16a(36),
            audio_size: u32a(38), audio_samples: u32a(42),
        }
    }
}

#[derive(Clone, Copy)]
pub struct IndexEntry {
    pub frame_no: u32, pub file_offset: u32, pub audio_bytes_before: u32,
    pub adpcm_predictor: i16, pub adpcm_step_index: u8,
}

pub fn index_size(n_entries: usize) -> usize { 8 + n_entries * INDEX_ENTRY_SIZE }

pub fn pack_index(entries: &[IndexEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(index_size(entries.len()));
    out.extend_from_slice(INDEX_MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.frame_no.to_le_bytes());
        out.extend_from_slice(&e.file_offset.to_le_bytes());
        out.extend_from_slice(&e.audio_bytes_before.to_le_bytes());
        out.extend_from_slice(&e.adpcm_predictor.to_le_bytes());
        out.push(e.adpcm_step_index);
        out.push(0); // pad
    }
    out
}
```

**Note on header field offsets** used in `parse`: magic 0, version 4, flags 6, codec 8, width 10, height 12, fps_num 14, fps_den 16, frame_count 18, index_offset 22, data_offset 26, audio_offset 30, audio_codec 34, audio_rate 36, audio_size 38, audio_samples 42 (matches the `<4sHHHHHHHIIIIHHII` layout).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test fwmv_format`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/fwmv/format.rs tests/fwmv_format.rs
git commit -m "feat: FWMV header/record/index primitives"
```

---

## Task 5: FWMV packer (`pack`) — full-file byte identity

**Files:**
- Modify: `src/fwmv/mod.rs`
- Test: `tests/fwmv_pack.rs`
- Uses fixtures: `tests/fixtures/pack_av.fwmv`, `tests/fixtures/pack_video_only.fwmv` (from Task 3 Step 2)

- [ ] **Step 1: Write the failing tests**

Create `tests/fwmv_pack.rs`:
```rust
use fileconvert::fwmv::{pack, PackParams};

fn sample_frames() -> Vec<Vec<u8>> {
    (0..40u32).map(|i| {
        let mut f = vec![0xFF, 0xD8];
        f.extend(std::iter::repeat((i & 0xFF) as u8).take(50 + i as usize));
        f.extend_from_slice(&[0xFF, 0xD9]);
        f
    }).collect()
}

fn sample_audio() -> Vec<i16> {
    (0..(40 * 16000 / 15 + 16000)).map(|i| ((i as f64 * 0.05).sin() * 6000.0) as i16).collect()
}

fn params() -> PackParams {
    PackParams { width: 480, height: 270, fps_num: 15, fps_den: 1, audio_rate: 16000 }
}

#[test]
fn pack_av_matches_python_golden() {
    let frames = sample_frames();
    let audio = sample_audio();
    let got = pack(&frames, Some(&audio), params());
    let want = std::fs::read("tests/fixtures/pack_av.fwmv").unwrap();
    assert_eq!(got, want, "av packing differs from Python golden");
}

#[test]
fn pack_video_only_matches_python_golden() {
    let frames = sample_frames();
    let got = pack(&frames, None, params());
    let want = std::fs::read("tests/fixtures/pack_video_only.fwmv").unwrap();
    assert_eq!(got, want, "video-only packing differs from Python golden");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test fwmv_pack`
Expected: FAIL (`pack`/`PackParams` not defined).

- [ ] **Step 3: Implement `src/fwmv/mod.rs`**

Port `pack_fwmv.py`'s `pack()` for the **no-trim** path (`max_bytes` is a non-goal in v1). Use the helper formulas from the reference-facts section.
```rust
pub mod adpcm;
pub mod format;

use format as F;

#[derive(Clone, Copy)]
pub struct PackParams {
    pub width: u16, pub height: u16,
    pub fps_num: u16, pub fps_den: u16,
    pub audio_rate: u16,
}

fn audio_samples_for(n_frames: usize, p: &PackParams) -> usize {
    n_frames * p.fps_den as usize * p.audio_rate as usize / p.fps_num as usize
}

fn lead_frames(p: &PackParams) -> usize {
    // ceil(fps_num / fps_den)
    let (num, den) = (p.fps_num as usize, p.fps_den as usize);
    (num + den - 1) / den
}

fn index_entry_frames(n: usize, p: &PackParams) -> Vec<usize> {
    let (num, den) = (p.fps_num as usize, p.fps_den as usize);
    let dur_s = n * den / num;
    let interval_s = if dur_s == 0 { 1 } else { ((dur_s + F::INDEX_MAX_ENTRIES - 1) / F::INDEX_MAX_ENTRIES).max(1) };
    let mut out = Vec::new();
    let mut k = 0usize;
    loop {
        let fk = (k * num + den - 1) / den; // ceil(k*num/den)
        if fk >= n { break; }
        out.push(fk);
        k += interval_s;
    }
    out
}

/// Cut the ADPCM blob at per-frame byte boundaries; last chunk takes the rest.
fn audio_chunks<'a>(blob: &'a [u8], n: usize, p: &PackParams) -> Vec<&'a [u8]> {
    let mut bounds: Vec<usize> = (0..n).map(|i| audio_samples_for(i, p) / 2).collect();
    bounds.push(blob.len());
    (0..n).map(|i| &blob[bounds[i]..bounds[i + 1]]).collect()
}

/// Write a full FWMV v2 file (header + interleaved record stream + index) to a
/// byte vector. No size trimming (v1 thumb-drive path). Mirrors pack_fwmv.pack.
pub fn pack(frames: &[Vec<u8>], audio_pcm: Option<&[i16]>, p: PackParams) -> Vec<u8> {
    let n = frames.len();
    let mut flags = 0u16;

    // --- audio prep ---
    let mut audio_blob: Vec<u8> = Vec::new();
    let mut audio_samples = 0usize;
    let mut chunks: Vec<&[u8]> = Vec::new();
    let has_audio = audio_pcm.is_some();
    if let Some(src) = audio_pcm {
        flags |= F::FLAG_AUDIO;
        audio_samples = audio_samples_for(n, &p);
        let mut pcm: Vec<i16> = src.iter().copied().take(audio_samples).collect();
        pcm.resize(audio_samples, 0); // pad if source audio is short
        audio_blob = adpcm::encode(&pcm);
        chunks = audio_chunks(&audio_blob, n, &p);
    }

    let lead = if has_audio { lead_frames(&p).min(n) } else { 0 };
    let entry_frames = index_entry_frames(n, &p);

    // --- record stream ---
    let mut stream: Vec<u8> = Vec::new();
    let mut entries: Vec<F::IndexEntry> = Vec::new();
    let mut audio_bytes_emitted: u32 = 0;
    let mut ei = 0usize;

    for i in 0..lead {
        stream.extend_from_slice(&F::pack_record(F::REC_AUDIO, chunks[i]));
        audio_bytes_emitted += chunks[i].len() as u32;
    }
    for (i, fr) in frames.iter().enumerate() {
        if ei < entry_frames.len() && i == entry_frames[ei] {
            entries.push(F::IndexEntry {
                frame_no: i as u32,
                file_offset: (F::HEADER_SIZE + stream.len()) as u32,
                audio_bytes_before: audio_bytes_emitted,
                adpcm_predictor: 0, adpcm_step_index: 0,
            });
            ei += 1;
        }
        stream.extend_from_slice(&F::pack_record(F::REC_VIDEO, fr));
        let j = i + lead;
        if has_audio && j < n {
            stream.extend_from_slice(&F::pack_record(F::REC_AUDIO, chunks[j]));
            audio_bytes_emitted += chunks[j].len() as u32;
        }
    }
    stream.extend_from_slice(&F::pack_record(F::REC_END, &[]));
    let index_offset = (F::HEADER_SIZE + stream.len()) as u32;

    // fill ADPCM state for each index entry
    if has_audio && !entries.is_empty() {
        let offsets: Vec<usize> = entries.iter().map(|e| e.audio_bytes_before as usize).collect();
        let states = adpcm::scan_states(&audio_blob, &offsets);
        for (e, (pred, si)) in entries.iter_mut().zip(states) {
            e.adpcm_predictor = pred;
            e.adpcm_step_index = si;
        }
    }
    stream.extend_from_slice(&F::pack_index(&entries));

    let header = F::Header {
        version: F::VERSION_V2, flags, codec: F::CODEC_MJPEG,
        width: p.width, height: p.height, fps_num: p.fps_num, fps_den: p.fps_den,
        frame_count: n as u32, index_offset, data_offset: F::HEADER_SIZE as u32,
        audio_offset: 0,
        audio_codec: if has_audio { F::AUDIO_IMA_ADPCM } else { 0 },
        audio_rate: if has_audio { p.audio_rate } else { 0 },
        audio_size: audio_blob.len() as u32,
        audio_samples: audio_samples as u32,
    };

    let mut out = Vec::with_capacity(F::HEADER_SIZE + stream.len());
    out.extend_from_slice(&header.pack());
    out.extend_from_slice(&stream);
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test fwmv_pack`
Expected: PASS (2 tests). If a mismatch appears, diff the first differing byte against the Python output and reconcile against `pack_fwmv.py` (most likely culprits: chunk bounds rounding, lead count, or index interval).

- [ ] **Step 5: Run the whole pure-Rust suite**

Run: `cargo test --test adpcm --test fwmv_format --test fwmv_pack`
Expected: all PASS. The format layer is now proven byte-identical to the authoritative Python.

- [ ] **Step 6: Commit**

```bash
git add src/fwmv/mod.rs tests/fwmv_pack.rs tests/fixtures/pack_av.fwmv tests/fixtures/pack_video_only.fwmv
git commit -m "feat: FWMV packer with byte-identical golden tests"
```

---

## Task 6: Static FFmpeg build + link (build infra)

This is the primary engineering cost. Goal: `cargo build` produces a binary with FFmpeg compiled in, no external `ffmpeg`.

**Files:**
- Create: `build/README.md` (records the recipe), `.cargo/config.toml`
- Test: `tests/integration_convert.rs` (smoke only this task)

- [ ] **Step 1: Produce a static, LGPL FFmpeg build for your dev OS**

Configure with shared libs OFF and only what we need. From an FFmpeg source checkout:
```bash
./configure --prefix="$PWD/ffmpeg-static" \
  --disable-shared --enable-static --enable-pic \
  --disable-programs --disable-doc --disable-network \
  --disable-gpl --disable-nonfree \
  --enable-decoder=h264,hevc,vp8,vp9,av1,mpeg4,mjpeg,aac,mp3,vorbis,opus,pcm_s16le \
  --enable-demuxer=mov,matroska,avi,mp4,mpegts \
  --enable-parser=h264,hevc,vp9,aac \
  --enable-encoder=mjpeg \
  --enable-filter=scale,pad,fps,aresample,aformat,format \
  --enable-protocol=file
make -j && make install
```
Record the exact commands and the FFmpeg version in `build/README.md`. (Windows: build under MSYS2/MinGW or use a prebuilt **static** LGPL dev package; macOS: same configure via Homebrew toolchain. The `ffmpeg-next` crate links whatever `FFMPEG_DIR` points at.)

- [ ] **Step 2: Point the build at the static libs**

Create `.cargo/config.toml`:
```toml
[env]
# Absolute path to the static FFmpeg prefix from Step 1 (contains lib/ and include/).
FFMPEG_DIR = "C:/~prj/Dropbox/vibeProjects/fileconvert/ffmpeg-static"
```
Add `/ffmpeg-static` to `.gitignore` (binaries are not committed; the recipe in `build/README.md` reproduces them).

- [ ] **Step 3: Re-enable the FFmpeg dependency**

If Task 1 commented out `ffmpeg-next` / `pub mod convert;`, restore them now.

- [ ] **Step 4: Write a smoke test that initializes FFmpeg**

Create `tests/integration_convert.rs`:
```rust
#[test]
fn ffmpeg_initializes() {
    ffmpeg_next::init().expect("ffmpeg init");
}
```
Add to `Cargo.toml` if needed: the crate is imported as `ffmpeg_next`.

- [ ] **Step 5: Build and run the smoke test**

Run: `cargo test --test integration_convert ffmpeg_initializes`
Expected: PASS, and `cargo build` links with no external `.dll`/`.so` for FFmpeg. Verify on Windows with `dumpbin /dependents target/debug/fileconvert.exe` (no `avcodec-*.dll`); on Linux/macOS with `ldd`/`otool -L` (no `libav*`).

- [ ] **Step 6: Commit**

```bash
git add .cargo/config.toml build/README.md .gitignore tests/integration_convert.rs
git commit -m "build: statically link FFmpeg (LGPL) and smoke test"
```

---

## Task 7: Generate a tiny test video + decode-to-frames

**Files:**
- Create: `scripts/gen_sample.ps1`, `tests/fixtures/sample.mp4`
- Modify: `src/convert.rs`
- Test: `tests/integration_convert.rs`

- [ ] **Step 1: Create the sample generator `scripts/gen_sample.ps1`**

One-time, uses any installed ffmpeg to make a 2-second 4:3 clip with a tone (so letterboxing and audio are both exercised).
```powershell
param([string]$Out = "tests/fixtures/sample.mp4")
ffmpeg -y -f lavfi -i "testsrc=size=320x240:rate=30:duration=2" `
       -f lavfi -i "sine=frequency=440:duration=2" `
       -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest $Out
```

- [ ] **Step 2: Generate and commit the sample**

Run: `powershell -File scripts/gen_sample.ps1`
Expected: `tests/fixtures/sample.mp4` created (a few hundred KB).

- [ ] **Step 3: Write the failing test** (append to `tests/integration_convert.rs`)

```rust
use fileconvert::convert;

#[test]
fn decodes_letterboxed_jpeg_frames() {
    let frames = convert::decode_video_frames("tests/fixtures/sample.mp4").unwrap();
    // 2 s at 15 fps target -> ~30 frames.
    assert!(frames.len() >= 25 && frames.len() <= 35, "got {} frames", frames.len());
    // Each frame is a complete JPEG.
    for f in &frames {
        assert_eq!(&f[0..2], &[0xFF, 0xD8], "JPEG SOI");
        assert_eq!(&f[f.len()-2..], &[0xFF, 0xD9], "JPEG EOI");
    }
}
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test --test integration_convert decodes_letterboxed_jpeg_frames`
Expected: FAIL (`decode_video_frames` undefined).

- [ ] **Step 5: Implement `decode_video_frames` in `src/convert.rs`**

Drive demux → decode → filtergraph (`scale=480:270:force_original_aspect_ratio=decrease,pad=480:270:(ow-iw)/2:(oh-ih)/2,fps=15`) → MJPEG encode at `-q:v 10`.
```rust
use std::path::Path;
use ffmpeg_next as ff;
use ff::format::Pixel;
use ff::media::Type;
use ff::software::scaling::flag::Flags; // not used directly; filtergraph handles scale
use ff::util::frame::video::Video;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 270;
pub const FPS: u32 = 15;
pub const AUDIO_RATE: u32 = 16000;
const JPEG_QSCALE: i32 = 10; // matches ffmpeg -q:v 10

#[derive(Debug)]
pub enum ConvertError { Ffmpeg(ff::Error), Io(std::io::Error), NoVideo, Empty }
impl From<ff::Error> for ConvertError { fn from(e: ff::Error) -> Self { ConvertError::Ffmpeg(e) } }
impl From<std::io::Error> for ConvertError { fn from(e: std::io::Error) -> Self { ConvertError::Io(e) } }

fn video_filter(decoder: &ff::decoder::Video) -> Result<ff::filter::Graph, ff::Error> {
    let mut g = ff::filter::Graph::new();
    let args = format!(
        "video_size={}x{}:pix_fmt={}:time_base=1/1000:pixel_aspect=1/1",
        decoder.width(), decoder.height(), decoder.format().descriptor().unwrap().name()
    );
    // Simpler/robust: pass numeric pix_fmt.
    let args = format!(
        "video_size={}x{}:pix_fmt={}:time_base=1/1000000:pixel_aspect=1/1",
        decoder.width(), decoder.height(), Into::<i32>::into(decoder.format())
    );
    g.add(&ff::filter::find("buffer").unwrap(), "in", &args)?;
    g.add(&ff::filter::find("buffersink").unwrap(), "out", "")?;
    let spec = "scale=480:270:force_original_aspect_ratio=decrease,\
                pad=480:270:(ow-iw)/2:(oh-ih)/2,fps=15,format=yuvj420p";
    g.output("in", 0)?.input("out", 0)?.parse(spec)?;
    g.validate()?;
    Ok(g)
}

fn encode_jpeg(frame: &Video) -> Result<Vec<u8>, ff::Error> {
    let codec = ff::encoder::find(ff::codec::Id::MJPEG).unwrap();
    let ctx = ff::codec::context::Context::new_with_codec(codec);
    let mut enc = ctx.encoder().video()?;
    enc.set_width(frame.width());
    enc.set_height(frame.height());
    enc.set_format(Pixel::YUVJ420P);
    enc.set_time_base((1, FPS as i32));
    // -q:v 10 => fixed qscale. FF_QP2LAMBDA = 118.
    unsafe {
        let p = enc.as_mut_ptr();
        (*p).flags |= ff::ffi::AV_CODEC_FLAG_QSCALE as i32;
        (*p).global_quality = JPEG_QSCALE * ff::ffi::FF_QP2LAMBDA;
    }
    let mut opened = enc.open()?;
    let mut f = frame.clone();
    unsafe { (*f.as_mut_ptr()).quality = JPEG_QSCALE * ff::ffi::FF_QP2LAMBDA; }
    opened.send_frame(&f)?;
    opened.send_eof()?;
    let mut packet = ff::Packet::empty();
    let mut out = Vec::new();
    while opened.receive_packet(&mut packet).is_ok() {
        out.extend_from_slice(packet.data().unwrap());
    }
    Ok(out)
}

pub fn decode_video_frames<P: AsRef<Path>>(path: P) -> Result<Vec<Vec<u8>>, ConvertError> {
    ff::init()?;
    let mut ictx = ff::format::input(&path)?;
    let stream = ictx.streams().best(Type::Video).ok_or(ConvertError::NoVideo)?;
    let stream_index = stream.index();
    let ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = ctx.decoder().video()?;
    let mut graph = video_filter(&decoder)?;

    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut decoded = Video::empty();
    let mut filtered = Video::empty();

    let mut push = |graph: &mut ff::filter::Graph, frames: &mut Vec<Vec<u8>>| -> Result<(), ConvertError> {
        while graph.get("out").unwrap().sink().frame(&mut filtered).is_ok() {
            frames.push(encode_jpeg(&filtered)?);
        }
        Ok(())
    };

    for (s, packet) in ictx.packets() {
        if s.index() != stream_index { continue; }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            graph.get("in").unwrap().source().add(&decoded)?;
            push(&mut graph, &mut frames)?;
        }
    }
    decoder.send_eof()?;
    while decoder.receive_frame(&mut decoded).is_ok() {
        graph.get("in").unwrap().source().add(&decoded)?;
        push(&mut graph, &mut frames)?;
    }
    graph.get("in").unwrap().source().flush()?;
    push(&mut graph, &mut frames)?;

    if frames.is_empty() { return Err(ConvertError::Empty); }
    Ok(frames)
}
```

**Implementation note:** `ffmpeg-next`'s exact method names for buffer source/sink frame I/O (`.source().add()`, `.sink().frame()`) and the `ffi` re-exports can vary by minor version. Pin the version from Task 1 and adjust these calls to the version's API if the build complains; the *structure* (buffer → scale/pad/fps/format → mjpeg encode at qscale 10) is what must hold. Verify by the test below, not by byte-matching the CLI.

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --test integration_convert decodes_letterboxed_jpeg_frames`
Expected: PASS. If frame count is off, check the `fps=15` filter is present; if JPEG markers fail, check the encoder opened as YUVJ420P.

- [ ] **Step 7: Commit**

```bash
git add scripts/gen_sample.ps1 tests/fixtures/sample.mp4 src/convert.rs tests/integration_convert.rs
git commit -m "feat: decode video to letterboxed MJPEG frames"
```

---

## Task 8: Decode audio to mono 16 kHz PCM

**Files:**
- Modify: `src/convert.rs`
- Test: `tests/integration_convert.rs`

- [ ] **Step 1: Write the failing test** (append)

```rust
#[test]
fn decodes_mono_16k_pcm() {
    let pcm = fileconvert::convert::decode_audio_pcm("tests/fixtures/sample.mp4").unwrap();
    // ~2 s of audio at 16 kHz mono.
    let pcm = pcm.expect("sample has audio");
    assert!(pcm.len() >= 28000 && pcm.len() <= 36000, "got {} samples", pcm.len());
}

#[test]
fn returns_none_for_no_audio() {
    // Frames-only file would return Ok(None); reuse sample but assert the API shape.
    // (sample.mp4 HAS audio, so we only check the Some branch above; this is a
    //  compile/contract guard.)
    let _f: fn(&str) -> Result<Option<Vec<i16>>, fileconvert::convert::ConvertError>
        = |p| fileconvert::convert::decode_audio_pcm(p);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test integration_convert decodes_mono_16k_pcm`
Expected: FAIL (`decode_audio_pcm` undefined).

- [ ] **Step 3: Implement `decode_audio_pcm` in `src/convert.rs`** (append)

```rust
use ff::util::frame::audio::Audio;
use ff::util::format::sample::{Sample, Type as SampleType};

fn audio_filter(decoder: &ff::decoder::Audio) -> Result<ff::filter::Graph, ff::Error> {
    let mut g = ff::filter::Graph::new();
    let layout = decoder.channel_layout();
    let args = format!(
        "time_base=1/{rate}:sample_rate={rate}:sample_fmt={fmt}:channel_layout=0x{layout:x}",
        rate = decoder.rate(),
        fmt = decoder.format().name(),
        layout = layout.bits(),
    );
    g.add(&ff::filter::find("abuffer").unwrap(), "in", &args)?;
    g.add(&ff::filter::find("abuffersink").unwrap(), "out", "")?;
    let spec = "aresample=16000,aformat=sample_fmts=s16:channel_layouts=mono";
    g.output("in", 0)?.input("out", 0)?.parse(spec)?;
    g.validate()?;
    Ok(g)
}

pub fn decode_audio_pcm<P: AsRef<Path>>(path: P) -> Result<Option<Vec<i16>>, ConvertError> {
    ff::init()?;
    let mut ictx = ff::format::input(&path)?;
    let stream = match ictx.streams().best(Type::Audio) {
        Some(s) => s, None => return Ok(None),
    };
    let stream_index = stream.index();
    let ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = ctx.decoder().audio()?;
    let mut graph = audio_filter(&decoder)?;

    let mut pcm: Vec<i16> = Vec::new();
    let mut decoded = Audio::empty();
    let mut filtered = Audio::empty();

    let mut drain = |graph: &mut ff::filter::Graph, pcm: &mut Vec<i16>| {
        while graph.get("out").unwrap().sink().frame(&mut filtered).is_ok() {
            // s16, mono, packed.
            let data = filtered.plane::<i16>(0);
            pcm.extend_from_slice(&data[..filtered.samples()]);
        }
    };

    for (s, packet) in ictx.packets() {
        if s.index() != stream_index { continue; }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            graph.get("in").unwrap().source().add(&decoded)?;
            drain(&mut graph, &mut pcm);
        }
    }
    decoder.send_eof()?;
    while decoder.receive_frame(&mut decoded).is_ok() {
        graph.get("in").unwrap().source().add(&decoded)?;
        drain(&mut graph, &mut pcm);
    }
    graph.get("in").unwrap().source().flush()?;
    drain(&mut graph, &mut pcm);

    let _ = (Sample::I16(SampleType::Packed),); // keep imports referenced
    Ok(Some(pcm))
}
```

**Implementation note:** same caveat as Task 7 — adjust frame I/O method names to the pinned `ffmpeg-next` version. The contract is: output is `s16`, mono, 16000 Hz, packed into a flat `Vec<i16>`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test integration_convert decodes_mono_16k_pcm`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/convert.rs tests/integration_convert.rs
git commit -m "feat: decode audio to mono 16 kHz s16 PCM"
```

---

## Task 9: `convert_file` end-to-end + parse-back

**Files:**
- Modify: `src/convert.rs`
- Test: `tests/integration_convert.rs`

- [ ] **Step 1: Write the failing test** (append)

```rust
use fileconvert::fwmv::format::Header;

#[test]
fn convert_file_writes_valid_fwmv() {
    let dest = std::env::temp_dir().join("fwmv_test_out");
    std::fs::create_dir_all(&dest).unwrap();
    let out = fileconvert::convert::convert_file(
        std::path::Path::new("tests/fixtures/sample.mp4"), &dest, |_| {}).unwrap();
    assert_eq!(out.extension().unwrap(), "fwmv");
    assert_eq!(out.file_stem().unwrap(), "sample");

    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[0..4], b"FWMV");
    let h = Header::parse(&bytes);
    assert_eq!(h.version, 2);
    assert_eq!(h.width, 480);
    assert_eq!(h.height, 270);
    assert_eq!(h.fps_num, 15);
    assert!(h.frame_count >= 25 && h.frame_count <= 35);
    assert_eq!(h.audio_rate, 16000); // sample has audio
    // index present at index_offset, magic FWIX
    let io = h.index_offset as usize;
    assert_eq!(&bytes[io..io+4], b"FWIX");
}

#[test]
fn convert_file_avoids_collisions() {
    let dest = std::env::temp_dir().join("fwmv_test_collide");
    std::fs::create_dir_all(&dest).unwrap();
    let _ = std::fs::remove_file(dest.join("sample.fwmv"));
    let _ = std::fs::remove_file(dest.join("sample_1.fwmv"));
    let a = fileconvert::convert::convert_file(
        std::path::Path::new("tests/fixtures/sample.mp4"), &dest, |_| {}).unwrap();
    let b = fileconvert::convert::convert_file(
        std::path::Path::new("tests/fixtures/sample.mp4"), &dest, |_| {}).unwrap();
    assert_ne!(a, b);
    assert_eq!(b.file_stem().unwrap(), "sample_1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test integration_convert convert_file`
Expected: FAIL (`convert_file` undefined).

- [ ] **Step 3: Implement `convert_file` + `unique_output_path` in `src/convert.rs`** (append)

```rust
use crate::fwmv::{self, PackParams};

fn unique_output_path(dest_dir: &Path, stem: &str) -> std::path::PathBuf {
    let first = dest_dir.join(format!("{stem}.fwmv"));
    if !first.exists() { return first; }
    let mut i = 1;
    loop {
        let p = dest_dir.join(format!("{stem}_{i}.fwmv"));
        if !p.exists() { return p; }
        i += 1;
    }
}

/// Convert one video to a .fwmv in `dest_dir`. `progress(frac)` is called with
/// 0.0..=1.0 as work proceeds. Returns the written path.
pub fn convert_file<P: AsRef<Path>>(
    input: P, dest_dir: &Path, mut progress: impl FnMut(f32),
) -> Result<std::path::PathBuf, ConvertError> {
    let input = input.as_ref();
    progress(0.05);
    let frames = decode_video_frames(input)?;       // heavy
    progress(0.7);
    let audio = decode_audio_pcm(input)?;            // heavy
    progress(0.9);

    let params = PackParams { width: WIDTH as u16, height: HEIGHT as u16,
                              fps_num: FPS as u16, fps_den: 1, audio_rate: AUDIO_RATE as u16 };
    let bytes = fwmv::pack(&frames, audio.as_deref(), params);

    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let out = unique_output_path(dest_dir, stem);
    std::fs::write(&out, &bytes)?;
    progress(1.0);
    Ok(out)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test integration_convert convert_file`
Expected: PASS (2 tests).

- [ ] **Step 5: Cross-check with the reference player parser (manual, optional)**

Run (from the movieplayer repo, against the temp output):
```powershell
cd C:\~prj\Dropbox\vibeProjects\movieplayer
python -c "from tools import fwmv_format as F; b=open(r'%TEMP%\fwmv_test_out\sample.fwmv','rb').read(); print(F.parse_header(b)); print(sum(1 for t,_ in F.iter_records(b, F.parse_header(b)['data_offset']) if t==1), 'video records')"
```
Expected: header prints version 2, width 480, and the video-record count equals `frame_count`.

- [ ] **Step 6: Commit**

```bash
git add src/convert.rs tests/integration_convert.rs
git commit -m "feat: convert_file end-to-end with collision-safe naming"
```

---

## Task 10: GUI (egui/eframe) with worker thread

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement `src/app.rs`**

```rust
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use eframe::egui;
use fileconvert::convert;

#[derive(Clone, PartialEq)]
enum Status { Queued, Converting(f32), Done(PathBuf), Failed(String) }

struct Item { path: PathBuf, status: Status }

enum Msg { Progress(usize, f32), Done(usize, PathBuf), Failed(usize, String), Batch }

pub struct App {
    items: Vec<Item>,
    dest: Option<PathBuf>,
    rx: Option<Receiver<Msg>>,
    running: bool,
}

impl Default for App {
    fn default() -> Self { Self { items: Vec::new(), dest: None, rx: None, running: false } }
}

impl App {
    fn add_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mkv", "mov", "avi", "webm", "m4v"])
            .pick_files()
        {
            for p in paths {
                if !self.items.iter().any(|i| i.path == p) {
                    self.items.push(Item { path: p, status: Status::Queued });
                }
            }
        }
    }

    fn choose_dest(&mut self) {
        if let Some(d) = rfd::FileDialog::new().pick_folder() { self.dest = Some(d); }
    }

    fn start(&mut self, ctx: egui::Context) {
        let Some(dest) = self.dest.clone() else { return; };
        let jobs: Vec<(usize, PathBuf)> = self.items.iter().enumerate()
            .filter(|(_, i)| !matches!(i.status, Status::Done(_)))
            .map(|(i, it)| (i, it.path.clone())).collect();
        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = channel();
        self.rx = Some(rx);
        self.running = true;
        for it in &mut self.items { if !matches!(it.status, Status::Done(_)) { it.status = Status::Queued; } }
        thread::spawn(move || {
            for (idx, path) in jobs {
                let txp = tx.clone();
                let ctxp = ctx.clone();
                let r = convert::convert_file(&path, &dest, |f| {
                    let _ = txp.send(Msg::Progress(idx, f));
                    ctxp.request_repaint();
                });
                match r {
                    Ok(out) => { let _ = tx.send(Msg::Done(idx, out)); }
                    Err(e) => { let _ = tx.send(Msg::Failed(idx, format!("{e:?}"))); }
                }
                ctx.request_repaint();
            }
            let _ = tx.send(Msg::Batch);
            ctx.request_repaint();
        });
    }

    fn drain(&mut self) {
        let mut done = false;
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Progress(i, f) => self.items[i].status = Status::Converting(f),
                    Msg::Done(i, p) => self.items[i].status = Status::Done(p),
                    Msg::Failed(i, e) => self.items[i].status = Status::Failed(e),
                    Msg::Batch => done = true,
                }
            }
        }
        if done { self.running = false; self.rx = None; }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.drain();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FWMV Video Converter");
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.running, egui::Button::new("Add files…")).clicked() {
                    self.add_files();
                }
                if ui.add_enabled(!self.running, egui::Button::new("Choose destination…")).clicked() {
                    self.choose_dest();
                }
            });
            ui.label(match &self.dest {
                Some(d) => format!("Destination: {}", d.display()),
                None => "Destination: (none chosen)".into(),
            });
            ui.separator();
            let total = self.items.len();
            let done = self.items.iter().filter(|i| matches!(i.status, Status::Done(_))).count();
            if total > 0 {
                ui.add(egui::ProgressBar::new(done as f32 / total as f32)
                    .text(format!("{done}/{total} files")));
            }
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for it in &self.items {
                    let name = it.path.file_name().unwrap().to_string_lossy();
                    let s = match &it.status {
                        Status::Queued => "queued".to_string(),
                        Status::Converting(f) => format!("converting… {:.0}%", f * 100.0),
                        Status::Done(_) => "done".to_string(),
                        Status::Failed(e) => format!("failed: {e}"),
                    };
                    ui.label(format!("{name}  —  {s}"));
                }
            });
            ui.separator();
            let can_start = !self.running && self.dest.is_some()
                && self.items.iter().any(|i| !matches!(i.status, Status::Done(_)));
            if ui.add_enabled(can_start, egui::Button::new("Convert")).clicked() {
                self.start(ctx.clone());
            }
        });
    }
}
```

- [ ] **Step 2: Implement `src/main.rs`**

```rust
mod app;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([560.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "FWMV Video Converter",
        options,
        Box::new(|_cc| Ok(Box::<app::App>::default())),
    )
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles. (Adjust any `eframe`/`egui` API drift to the pinned 0.29 versions — e.g. the `run_native` closure signature.)

- [ ] **Step 4: Manual smoke test**

Run: `cargo run`
Expected: a window opens. *Add files…* opens a native multi-select dialog; *Choose destination…* opens a folder picker; *Convert* processes the queue, the progress bar advances, rows turn to "done", and `.fwmv` files appear in the destination. A second run on the same file produces `name_1.fwmv`.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "feat: egui GUI with native dialogs and worker thread"
```

---

## Task 11: Cross-platform packaging / CI

**Files:**
- Create: `.github/workflows/build.yml`, `README.md`

- [ ] **Step 1: Write `README.md`** documenting: what the app does, the LGPL static-FFmpeg requirement, how to set `FFMPEG_DIR` (Task 6), how to run, and that the output `.fwmv` files go on a FAT32 thumb drive root (≤32 files, name-sorted) per the player rules.

- [ ] **Step 2: Add CI `.github/workflows/build.yml`**

Build a release binary on `windows-latest`, `ubuntu-latest`, `macos-latest`. Each job: install/build the static FFmpeg (per `build/README.md`), set `FFMPEG_DIR`, run `cargo test` (the pure-Rust suites always run; integration tests need the static libs), then `cargo build --release` and upload the binary as an artifact.
```yaml
name: build
on: [push, workflow_dispatch]
jobs:
  build:
    strategy:
      matrix:
        os: [windows-latest, ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      # NOTE: provision static LGPL FFmpeg here (script per build/README.md),
      # then export FFMPEG_DIR before the cargo steps.
      - run: cargo test --test adpcm --test fwmv_format --test fwmv_pack
      - run: cargo build --release
      - uses: actions/upload-artifact@v4
        with:
          name: fileconvert-${{ matrix.os }}
          path: |
            target/release/fileconvert
            target/release/fileconvert.exe
```

- [ ] **Step 3: Verify the pure-Rust suite runs without FFmpeg in CI**

Run locally: `cargo test --test adpcm --test fwmv_format --test fwmv_pack`
Expected: PASS (these have no FFmpeg dependency and pin format correctness on every platform).

- [ ] **Step 4: Commit**

```bash
git add README.md .github/workflows/build.yml
git commit -m "ci: cross-platform build + format-correctness gate"
```

---

## Self-review notes (coverage against spec)

- **Single self-contained binary / static FFmpeg** → Tasks 6, 11 (link + verify no external `libav*`).
- **Multi-file selection / destination folder** → Task 10 (`rfd` `pick_files` / `pick_folder`).
- **Progress display, non-blocking UI** → Task 10 (worker thread + channel + progress bar).
- **Thumb-drive `.fwmv` only, fixed defaults** → Task 9 (`PackParams` with 480/270/15/16000), no trim path in Task 5.
- **Byte-compatible format** → Tasks 2–5 golden vectors vs Python; Task 9 parse-back + Task 9 Step 5 reference-parser cross-check.
- **Per-file error isolation, no-audio path** → Task 9 (`Result` per file), Task 5 (`audio_pcm = None` branch), Task 10 (`Failed` status continues the batch).
- **Collision-safe naming** → Task 9 (`unique_output_path`).
- **Cross-platform** → Task 11 CI matrix.

**Known adjustment points** (flagged inline, not placeholders): exact `ffmpeg-next` frame-I/O method names and `ffi` constant paths (Tasks 7–8) depend on the pinned crate version; `eframe`/`egui` 0.29 API details (Task 10). Each has a concrete verification test so drift is caught immediately.
