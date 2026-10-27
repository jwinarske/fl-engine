#!/usr/bin/env bash
# Builds the prebuilt Dart VM artifacts fl-dart-sys links, from a gclient
# checkout of dart-lang/sdk at the pinned revision.
#
#   build.sh <sdk-checkout> <target> <mode> <out-dir>
#
#   target: x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | riscv64gc-unknown-linux-gnu
#   mode:   jit (x86_64 only) | aot
#
# Writes <out-dir>/fl-dart-prebuilt-<version>-<target>-<mode>.tar.xz and a
# .sha256 beside it. The tarball unpacks over third_party/dart-prebuilt/, in
# the layout third_party/dart-prebuilt/README.md describes.
#
# This is the one place C++ is compiled for fl-engine (brief §4.1). It needs
# what a Dart SDK build needs -- depot_tools on PATH and a synced checkout --
# and nothing cargo knows about.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
sdk="$(cd "$1" && pwd)"
target="$2"
mode="$3"
out="$(mkdir -p "$4" && cd "$4" && pwd)"

die() { echo "build.sh: $*" >&2; exit 1; }

# The pin is read from the checked-in VERSION, never restated here.
pinned_dir="$(ls -d "$repo"/third_party/dart-prebuilt/*/ | head -n1)"
version="$(basename "$pinned_dir")"
field() { sed -n "s/^$1=//p" "$pinned_dir/VERSION"; }
revision="$(field revision)"
snapshot_hash="$(field snapshot_hash)"

head="$(git -C "$sdk" rev-parse HEAD)"
[[ "$head" == "$revision" ]] || die "checkout is at $head, the pin is $revision"
actual_hash="$(cd "$sdk" && python3 -c 'import sys; sys.path.insert(0, "tools"); import make_version; print(make_version.MakeSnapshotHashString())')"
[[ "$actual_hash" == "$snapshot_hash" ]] || die "snapshot hash is $actual_hash, VERSION says $snapshot_hash"

case "$target" in
  x86_64-unknown-linux-gnu) arch=x64 ;;
  aarch64-unknown-linux-gnu) arch=arm64 ;;
  riscv64gc-unknown-linux-gnu) arch=riscv64 ;;
  *) die "unsupported target $target" ;;
esac
case "$mode" in
  # Release, not debug: the non-product JIT. Product is the configuration the
  # stock SDK's AOT snapshots declare in their feature string, and the VM
  # refuses a snapshot whose features differ from its own.
  jit) [[ "$arch" == x64 ]] || die "jit is an x86_64 dev-host mode (brief §3)"; build_mode=release ;;
  aot) build_mode=product ;;
  *) die "mode must be jit or aot" ;;
esac

# Graft the shim into the checkout and make it reachable from the root, since
# gn only evaluates files the default graph references.
rm -rf "$sdk/fl_dart"
cp -r "$here/fl_dart" "$sdk/fl_dart"
grep -q '"//fl_dart:all"' "$sdk/BUILD.gn" || cat >> "$sdk/BUILD.gn" <<'GN'

# Added by fl-engine's tools/dart-prebuilt/build.sh.
group("fl_dart") {
  deps = [ "//fl_dart:all" ]
}
GN

(cd "$sdk" && python3 tools/build.py --no-rbe --mode "$build_mode" --arch "$arch" "fl_dart_$mode")

conf="$(cd "$sdk" && python3 -c "import sys; sys.path.insert(0, 'tools'); import utils; print(utils.GetBuildConf('$build_mode', '$arch'))")"
archive="$(find "$sdk/out/$conf" -name "libfl_dart_$mode.a" -print -quit)"
[[ -n "$archive" ]] || die "no libfl_dart_$mode.a under out/$conf"

# The SDK's toolchain is clang with libc++, linked statically into every SDK
# binary (`-static-libstdc++` in build/config/linux/BUILD.gn): dartaotruntime
# has no C++ runtime among its NEEDED entries. A consumer of our archive gets
# the same by merging the toolchain's libc++ and libc++abi into it here, so
# the archive carries its own C++ runtime and link.txt names none. Unwinding
# stays with the consumer's libgcc_s, which is what Rust links anyway; the
# toolchain's libunwind is deliberately left out so there is one unwinder.
clang_triple="${target%%-*}-unknown-linux-gnu"
clang="$(find "$sdk/buildtools" -type f -path "*/clang/bin/clang" -print -quit)"
[[ -n "$clang" ]] || die "no clang under $sdk/buildtools"
# The toolchain knows where its own runtime libraries are; asking it is the
# answer that survives a package relayout. The find is a fallback for a clang
# that answers with a bare name (what --print-file-name does for a miss).
cxx_a="$("$clang" --target="$clang_triple" --print-file-name=libc++.a)"
if [[ ! -f "$cxx_a" ]]; then
  cxx_a="$(find "$sdk/buildtools" -type f -name libc++.a -path "*${clang_triple%%-*}*" -print -quit)"
fi
if [[ ! -f "$cxx_a" ]]; then
  echo "build.sh: libc++ candidates under buildtools:" >&2
  find "$sdk/buildtools" -name 'libc++*' >&2 || true
  die "no static libc++ for $clang_triple"
fi
cxx_lib="$(dirname "$cxx_a")"
[[ -f "$cxx_lib/libc++abi.a" ]] || die "$cxx_lib has libc++.a but no libc++abi.a"
echo "build.sh: using libc++ from $cxx_lib"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
dest="$stage/$version"
lib="$dest/lib/$target/$mode"
mkdir -p "$lib" "$dest/include"
# An MRI script merges whole archives without extracting members whose names
# collide across them.
llvm_ar="$(find "$sdk/buildtools" -type f -name llvm-ar -print -quit)"
[[ -n "$llvm_ar" ]] || die "no llvm-ar under $sdk/buildtools"
"$llvm_ar" -M <<MRI
CREATE $lib/libfl_dart_$mode.a
ADDLIB $archive
ADDLIB $cxx_lib/libc++.a
ADDLIB $cxx_lib/libc++abi.a
SAVE
END
MRI
cp "$sdk"/runtime/include/{dart_api.h,dart_native_api.h,dart_tools_api.h,dart_version.h} "$dest/include/"
cp "$here/fl_dart/fl_dart_io.h" "$dest/include/"
cp "$pinned_dir/VERSION" "$dest/VERSION"
cat > "$lib/link.txt" <<LINK
# Written by tools/dart-prebuilt/build.sh from $conf at $revision.
static=fl_dart_$mode
# No cxx= line: the archive carries its own libc++ and libc++abi, merged in by
# build.sh, the way every SDK binary links its C++ runtime statically.
system=dl
system=pthread
system=m
LINK

name="fl-dart-prebuilt-$version-$target-$mode"
# Deterministic tarball: the checksum names the content, not the build time.
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
  -C "$stage" -cJf "$out/$name.tar.xz" "$version"
(cd "$out" && sha256sum "$name.tar.xz" > "$name.tar.xz.sha256")
echo "wrote $out/$name.tar.xz"
