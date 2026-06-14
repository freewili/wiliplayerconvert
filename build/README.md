# Building fileconvert (static FFmpeg link)

The app links FFmpeg **statically** so the shipped binary has no external FFmpeg
DLL dependency. This document records how the local Windows build environment was
set up so it can be reproduced (and so CI can mirror it — see Task 11).

## What's installed on the dev machine

| Component | Version | Source | Purpose |
|-----------|---------|--------|---------|
| Rust (MSVC host) | 1.95 | rustup | `x86_64-pc-windows-msvc` |
| Visual Studio | 18 Professional | — | MSVC toolchain + Windows SDK (linker, headers) |
| LLVM / libclang | 22.1.7 | `winget install LLVM.LLVM` | bindgen needs `libclang.dll` |
| vcpkg | 2026-04-08 | `C:\Users\dave\vcpkg` | builds + provides static FFmpeg |
| FFmpeg (static, LGPL) | 8.1.1 | `vcpkg install ffmpeg:x64-windows-static-md` | the codecs |

## One-time setup

```powershell
# 1) Static FFmpeg (LGPL by default — no -gpl). static-md = static libs, dynamic CRT,
#    which matches Rust's default MSVC CRT linkage.
& "$env:VCPKG_ROOT\vcpkg.exe" install ffmpeg:x64-windows-static-md --clean-after-build

# 2) libclang for bindgen
winget install --id LLVM.LLVM --silent --accept-package-agreements --accept-source-agreements

# 3) A standalone pkgconf for ffmpeg-sys discovery (the MinGW pkg-config on PATH
#    would re-trigger clang's MinGW-header detection, so we use a native one).
Copy-Item "$env:VCPKG_ROOT\buildtrees\pkgconf\x64-windows-rel\pkgconf.exe"   "$env:VCPKG_ROOT\pkgconf.exe" -Force
Copy-Item "$env:VCPKG_ROOT\buildtrees\pkgconf\x64-windows-rel\pkgconf-7.dll" "$env:VCPKG_ROOT\pkgconf-7.dll" -Force
```

## The crate

`Cargo.toml` uses `ffmpeg-the-third` (v5, supports FFmpeg 8.x) aliased to the
`ffmpeg-next` import name. `ffmpeg-sys-the-third` discovers the libs via
`pkg-config` reading the vcpkg `.pc` files.

## Build environment (`.cargo/config.toml`)

`.cargo/config.toml` (committed, machine-specific paths) sets:

- `LIBCLANG_PATH` → LLVM `bin` (bindgen).
- `VCPKG_ROOT` → the vcpkg tree.
- `BINDGEN_EXTRA_CLANG_ARGS = --target=x86_64-pc-windows-msvc` → clang in MSVC mode.
- `PKG_CONFIG` → the standalone `pkgconf.exe` (NOT the MinGW one on PATH).
- `PKG_CONFIG_PATH` → vcpkg's `lib/pkgconfig`.
- `PKG_CONFIG_ALL_STATIC = 1` → emit the full transitive static lib set.
- `[build] target-dir` → a path **outside** the Dropbox-synced repo (Dropbox locks
  files in `target/` mid-compile → `os error 32`).

If you move machines, update these paths and the VS path in `scripts/dev-cargo.ps1`.

## How to build / test

Do **not** run `cargo` directly from Git Bash — `C:\msys64\mingw64\bin` on PATH makes
bindgen's clang grab MinGW headers and fail. Use the wrapper, which strips msys64 from
PATH and loads the MSVC environment:

```bash
powershell -ExecutionPolicy Bypass -File scripts/dev-cargo.ps1 build
powershell -ExecutionPolicy Bypass -File scripts/dev-cargo.ps1 test
```

The pure-Rust format tests have no FFmpeg dependency and run with plain cargo too:

```bash
cargo test --test adpcm --test fwmv_format --test fwmv_pack
```

## Verifying the static link

```powershell
dumpbin /dependents <target-dir>\debug\fileconvert.exe
```
Expect only Windows system DLLs + `VCRUNTIME140.dll` — **no `avcodec`/`avformat`/`avutil`**.
