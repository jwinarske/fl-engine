#!/usr/bin/env bash
# Exit criterion 8 (M1, M3, M4): fl-dart-sys builds, and fl-dart links and
# runs, with no C++ toolchain present; and with the KMS platform, whose
# system libraries are linked, never built. Run as root in a bare
# ubuntu:24.04 container (ci.yml's no-cxx job); installs only what a Rust
# build needs, the C linker driver rustc calls and the KMS platform's four
# libraries, and proves the C++ compilers are absent before building.
#
#   ci/no-cxx.sh        # GITHUB_TOKEN, if set, is used for the fetch
#
# libstdc++6 itself is in every Ubuntu base (apt links it); that is a runtime
# library, not a toolchain, and the binary is checked not to need it.
set -euo pipefail
cd "$(dirname "$0")/.."

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  ca-certificates curl gcc libc6-dev xz-utils unzip binutils \
  pkg-config libseat-dev libinput-dev libudev-dev libxkbcommon-dev >/dev/null

for cxx in g++ c++ clang++ clang; do
  if command -v "$cxx" >/dev/null; then
    echo "no-cxx: $cxx is installed ($(command -v "$cxx")); this lane must not have it" >&2
    exit 1
  fi
done
echo "no-cxx: no g++, c++, clang++ or clang on PATH"

toolchain="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
[[ -n "$toolchain" ]] || { echo "no-cxx: no channel in rust-toolchain.toml" >&2; exit 1; }
export PATH="$HOME/.cargo/bin:$PATH"
# A toolchain already present (mounted in, for a local run) is used as is;
# rust-toolchain.toml still pins the version cargo runs.
if ! command -v cargo >/dev/null; then
  curl -fsSL --retry 3 https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain "$toolchain"
fi

cargo xtask fetch-prebuilt --target x86_64-unknown-linux-gnu --mode aot

base="https://storage.googleapis.com/dart-archive/channels/stable/release/3.13.7/sdk"
sdk="$(mktemp -d)"
curl -fsSL --retry 3 -o "$sdk/sdk.zip" "$base/dartsdk-linux-x64-release.zip"
want="$(curl -fsSL --retry 3 "$base/dartsdk-linux-x64-release.zip.sha256sum" | cut -d' ' -f1)"
echo "$want  $sdk/sdk.zip" | sha256sum -c -
unzip -q "$sdk/sdk.zip" -d "$sdk"
export FL_DART_SDK_BIN="$sdk/dart-sdk/bin/dart"

cargo test -p fl-dart-cli

# The archive carries its own libc++; the binary must need no C++ runtime.
needed="$(readelf -d target/debug/fl-dart | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')"
echo "no-cxx: fl-dart needs: $(echo "$needed" | tr '\n' ' ')"
if echo "$needed" | grep -E 'libstdc\+\+|libc\+\+' >/dev/null; then
  echo "no-cxx: fl-dart links a C++ runtime dynamically" >&2
  exit 1
fi

# The KMS platform (brief M4 exit criterion 8): libseat, libinput, libudev
# and libxkbcommon are the system's, found by pkg-config and linked. No
# crate of the build compiles C: none has `cc`, `cmake` or `bindgen` to do
# it with, and gcc above is the linker driver alone.
builders="$(cargo tree -p fl-dart-cli --features kms -e normal,build --prefix none \
  | sed 's/ .*//' | sort -u | grep -Ex 'cc|cmake|bindgen|autotools|meson' || true)"
if [[ -n "$builders" ]]; then
  echo "no-cxx: the KMS build has a crate that builds C: $(echo "$builders" | tr '\n' ' ')" >&2
  exit 1
fi
cargo build -p fl-dart-cli --features kms
needed="$(readelf -d target/debug/fl-dart | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')"
echo "no-cxx: fl-dart with kms needs: $(echo "$needed" | tr '\n' ' ')"
if echo "$needed" | grep -E 'libstdc\+\+|libc\+\+' >/dev/null; then
  echo "no-cxx: fl-dart with kms links a C++ runtime dynamically" >&2
  exit 1
fi
for lib in libseat libinput libudev libxkbcommon; do
  echo "$needed" | grep -q "^$lib\.so" \
    || { echo "no-cxx: fl-dart with kms does not link the system's $lib" >&2; exit 1; }
done
echo "no-cxx: built, linked and ran with no C++ toolchain"
