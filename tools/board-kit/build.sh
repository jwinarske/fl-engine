#!/usr/bin/env bash
# Builds the board kit: what exit criterion 2 needs on a real board, so the
# board session is copy, unpack, run.
#
#   build.sh <target> <dart> <out-dir>
#
#   target: aarch64-unknown-linux-gnu | riscv64gc-unknown-linux-gnu
#   dart:   the pinned stock SDK's dart, which cross-compiles the programs
#
# The cargo target linker must be set (CARGO_TARGET_<T>_LINKER), as cross.yml
# does. Writes <out-dir>/fl-dart-board-kit-<target>.tar.xz: fl-dart (release),
# the L2 programs as AOT snapshots for the target, a snapshot with its SDK
# hash overwritten (criterion 5), run.sh, and MANIFEST with the commit and
# every file's sha256.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
target="$1"
dart="$2"
out="$(mkdir -p "$3" && cd "$3" && pwd)"

case "$target" in
  aarch64-unknown-linux-gnu) arch=arm64 ;;
  riscv64gc-unknown-linux-gnu) arch=riscv64 ;;
  *) echo "build.sh: unsupported target $target" >&2; exit 1 ;;
esac

name="fl-dart-board-kit-$target"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
kit="$stage/$name"
mkdir -p "$kit"

(cd "$repo" && cargo build --release -p fl-dart-cli --target "$target")
cp "$repo/target/$target/release/fl-dart" "$kit/"

for prog in hello loop ffi throws burst; do
  "$dart" compile aot-snapshot --target-os linux --target-arch "$arch" \
    "$repo/crates/fl-dart-cli/tests/dart/$prog.dart" -o "$kit/$prog.aot"
done

# Criterion 5: the same snapshot with its producer hash overwritten, as the
# L2 version-mismatch test makes it. The hash is the pin's, from VERSION.
hash="$(sed -n 's/^snapshot_hash=//p' "$repo"/third_party/dart-prebuilt/*/VERSION)"
python3 - "$kit/hello.aot" "$kit/mismatch.aot" "$hash" <<'PY'
import sys
src, dst, h = sys.argv[1], sys.argv[2], sys.argv[3].encode()
b = bytearray(open(src, "rb").read())
i = b.find(h)
if i < 0:
    sys.exit(f"snapshot hash {h.decode()} not in {src}")
b[i:i + len(h)] = b"f" * len(h)
open(dst, "wb").write(b)
PY

cp "$here/run.sh" "$kit/"
{
  echo "commit=$(git -C "$repo" rev-parse HEAD)"
  echo "target=$target"
  echo "dart=$("$dart" --version 2>&1)"
  (cd "$kit" && sha256sum fl-dart ./*.aot run.sh)
} > "$kit/MANIFEST"

tar --sort=name --owner=0 --group=0 --numeric-owner -C "$stage" -cJf "$out/$name.tar.xz" "$name"
echo "wrote $out/$name.tar.xz"
