# Wrapper that runs cargo with the environment needed to build against the
# statically-linked FFmpeg on this Windows machine.
#
# Why this is needed:
#   * Git Bash puts C:\msys64\mingw64\bin on PATH. If clang (used by bindgen)
#     sees that MinGW toolchain it tries to use MinGW headers and fails. We strip
#     msys64 from PATH so clang falls back to the MSVC/Windows SDK headers.
#   * vcvars64 loads the MSVC toolchain environment for the final link.
#
# All other settings (LIBCLANG_PATH, PKG_CONFIG, PKG_CONFIG_PATH,
# PKG_CONFIG_ALL_STATIC, VCPKG_ROOT, target-dir) live in .cargo/config.toml.
#
# Usage:
#   powershell -File scripts/dev-cargo.ps1 build
#   powershell -File scripts/dev-cargo.ps1 test
#   powershell -File scripts/dev-cargo.ps1 test --test integration_convert
#
# The VS path below is machine-specific; adjust for your install (see build/README.md).
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArgs)

$ErrorActionPreference = 'Stop'
$env:PATH = ($env:PATH -split ';' | Where-Object { $_ -notmatch 'msys64' }) -join ';'
$vcvars = "C:\Program Files\Microsoft Visual Studio\18\Professional\VC\Auxiliary\Build\vcvars64.bat"
$joined = $CargoArgs -join ' '
& cmd /c "`"$vcvars`" >nul 2>&1 && cargo $joined"
exit $LASTEXITCODE
