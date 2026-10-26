#!/usr/bin/env bash
# Reproduce the NES-frame-on-band10pro build end to end.
#
# Clones the pinned vendor repositories if missing, applies the local x-nes
# patch, builds Canopus, regenerates the test ROM, builds/verifies/packages the
# module, and runs the host-side frame check. It does NOT touch the base repo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$ROOT/vendor"
TOOLS="$ROOT/tools/bin"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.0}"
export CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

CANOPUS_SHA=4928f1eb70594263292c32ffc531a7cad3aea555
XNES_SHA=ca4c3955b8a442fe25ee62abd57fca6849d1e70b
LYRA_SHA=8b228e37157766e0ea7e3d31a9d0ca2aaf053d7f
MIBIN_SHA=5e4195ce91b9d8810a7a2233c96b4cba44184adc

clone_at() {
  local url="$1" dir="$2" sha="$3"
  if [ ! -d "$VENDOR/$dir/.git" ]; then
    git clone --depth 1 "$url" "$VENDOR/$dir"
  fi
  git -C "$VENDOR/$dir" fetch --depth 1 origin "$sha" >/dev/null 2>&1 || true
  git -C "$VENDOR/$dir" checkout --detach "$sha" >/dev/null 2>&1
}

mkdir -p "$VENDOR" "$TOOLS" "$ROOT/build/testroms" "$ROOT/build/host-render"
clone_at https://github.com/AstralSightStudios/Canopus.git Canopus "$CANOPUS_SHA"
clone_at https://github.com/ChaoswareHQ/x-nes.git x-nes "$XNES_SHA"
clone_at https://github.com/AstralSightStudios/Canopus-App-LyraPlayer.git Canopus-App-LyraPlayer "$LYRA_SHA"
clone_at https://github.com/zhy8388608/Mi8WfBinTool.git Mi8WfBinTool "$MIBIN_SHA"

# Local x-nes patch (idempotent: skip if already applied).
if git -C "$VENDOR/x-nes" apply --check "$ROOT/patches/x-nes-043.patch" 2>/dev/null; then
  git -C "$VENDOR/x-nes" apply "$ROOT/patches/x-nes-043.patch"
elif git -C "$VENDOR/x-nes" apply --reverse --check "$ROOT/patches/x-nes-043.patch" 2>/dev/null; then
  echo "x-nes patch already applied"
else
  echo "x-nes patch does not apply cleanly" >&2
  exit 1
fi

# Canopus CLI.
cargo build --release -p canopus-cli --manifest-path "$VENDOR/Canopus/Cargo.toml"
ln -sfn "$VENDOR/Canopus/target/release/canopus" "$TOOLS/canopus"

# Test ROM (self-authored, no copyrighted content).
python3 "$ROOT/scripts/gen_test_rom.py"

# Module build + verifier + package.
cd "$ROOT/nesband10pro"
"$TOOLS/canopus" build
"$TOOLS/canopus" package
"$TOOLS/canopus" package verify dist/nesband10pro-0.1.0.canopus

# Host-side frame check, reusing the device conversion code.
cd "$ROOT/probe/host-render"
cargo build --release
cd "$ROOT"
./probe/host-render/target/release/host-render assets/pattern.nes build/host-render
for frame in 1 2 8; do
  python3 scripts/ppm_to_png.py "build/host-render/frame$frame.ppm" "build/host-render/frame$frame.png"
done

sha256sum "$ROOT/nesband10pro/build/xiaomi-band-10-pro-3.101.043/nesband10pro.elf" \
          "$ROOT/nesband10pro/dist/nesband10pro-0.1.0.canopus" \
          "$ROOT/assets/pattern.nes"
