#!/usr/bin/env bash
# Builds a Flutter app's libapp.so the way `flutter build linux --release`
# does, without the desktop embedding around it: frontend_server over the
# product platform (--aot --tfa), then the release gen_snapshot to an ELF.
#
#   build.sh <flutter-root> <app-dir> <out-dir> [target | kernel]
#
#   target: x86_64-unknown-linux-gnu (default) | aarch64-unknown-linux-gnu
#           | riscv64gc-unknown-linux-gnu
#           (the other two cross-compiled on an x64 host: Flutter's
#           linux-arm64 gen_snapshot is itself an arm64 binary and it has
#           none for riscv64, so the Dart SDK's x64-to-target one, the same
#           VM revision, is used instead)
#   kernel: <out-dir>/<app>.dill instead, for fl-dart's JIT build: the
#           non-product platform, linked in, as `flutter run` compiles in
#           debug mode but in one file. Architecture-independent.
#
# Either way the app's asset bundle goes to <out-dir>/<app>.assets: what a
# Linux bundle keeps in data/flutter_assets (FontManifest.json, fonts, the
# app's own assets, the code assets its packages' build hooks make), from
# `flutter build bundle`. fl-dart run-app finds it
# beside the program.
#
# And <out-dir>/<app>/ is the two laid out as `flutter build linux` lays out
# a bundle, hard links where the file system allows: lib/libapp.so (or
# lib/app.dill) and the code assets beside it, data/flutter_assets, and in
# it the version.json that build writes from the pubspec's name and version. fl-dart run-app takes the
# directory. A plugin reading beside the running executable
# (package_info_plus) finds the app's own files when fl-dart is copied into
# the directory and run from there.
#
# The Flutter release must be the one that pins the VM (its DEPS
# dart_revision equals third_party/dart-prebuilt/*/VERSION's revision);
# anything else makes a snapshot fl-dart refuses by hash. Writes
# <out-dir>/<app>.so.
set -euo pipefail

flutter_root="$(cd "$1" && pwd)"
app="$(cd "$2" && pwd)"
out="$(mkdir -p "$3" && cd "$3" && pwd)"
target="${4:-x86_64-unknown-linux-gnu}"
name="$(basename "$app")"

case "$target" in
  x86_64-unknown-linux-gnu) engine=linux-x64-release; platform=linux-x64 ;;
  aarch64-unknown-linux-gnu) engine=linux-arm64-release; cross=arm64; platform=linux-arm64 ;;
  riscv64gc-unknown-linux-gnu) engine=; cross=riscv64; platform=linux-riscv64 ;;
  kernel) engine=; platform=linux-x64 ;;
  *) echo "build.sh: unsupported target $target" >&2; exit 1 ;;
esac

cache="$flutter_root/bin/cache"
dart="$cache/dart-sdk/bin/dartaotruntime"
fes="$cache/dart-sdk/bin/snapshots/frontend_server_aot.dart.snapshot"
(cd "$app" && "$flutter_root/bin/flutter" pub get >/dev/null)

# The asset bundle, both modes. Release, so no kernel_blob.bin rides along.
# For the target's platform: the build hooks make its code assets, which go
# to native_assets/linux/ under the target's key in NativeAssetsManifest.json
# (the kernel is run on the host).
rm -rf "$out/$name.assets"
(cd "$app" && "$flutter_root/bin/flutter" build bundle --release --no-pub \
  --target-platform "$platform" --asset-dir "$out/$name.assets" >/dev/null)
# version.json, as flutter_tools' getVersionInfo writes it for a Linux
# build: the app's name, and its version and build number where the
# pubspec states them (`version: 1.2.3+4`). `build bundle` writes none.
app_name="$(sed -n 's/^name:[[:space:]]*\([A-Za-z0-9_]*\).*/\1/p' "$app/pubspec.yaml" | head -n 1)"
app_version="$(sed -n 's/^version:[[:space:]]*\([0-9A-Za-z.+-]*\).*/\1/p' "$app/pubspec.yaml" | head -n 1)"
version_json="{\"app_name\":\"$app_name\""
if [[ -n "$app_version" ]]; then
  version_json+=",\"version\":\"${app_version%%+*}\""
  [[ "$app_version" == *+* ]] && version_json+=",\"build_number\":\"${app_version#*+}\""
fi
version_json+=",\"package_name\":\"$app_name\"}"
printf '%s' "$version_json" > "$out/$name.assets/version.json"
# The bundle: <out>/<app>/lib/<program> and data/flutter_assets, linked to
# the two above rather than copied where the file system allows.
bundle() {
  rm -rf "${out:?}/$name"
  mkdir -p "$out/$name/lib" "$out/$name/data"
  cp -al "$out/$name.assets" "$out/$name/data/flutter_assets" 2>/dev/null ||
    cp -a "$out/$name.assets" "$out/$name/data/flutter_assets"
  ln -f "$1" "$out/$name/lib/$2" 2>/dev/null || cp -f "$1" "$out/$name/lib/$2"
  # The code assets in lib/ too, where `flutter build linux` puts them: a
  # bare name given to DynamicLibrary.open is found there through fl-dart's
  # RUNPATH, $ORIGIN/lib, as through the engine library's in a stock bundle.
  local asset
  for asset in "$out/$name.assets/native_assets/linux/"*; do
    [[ -f "$asset" ]] || continue
    ln -f "$asset" "$out/$name/lib/" 2>/dev/null || cp -f "$asset" "$out/$name/lib/"
  done
}

# The Dart plugin registrant `flutter build bundle` generated, compiled in as
# flutter_tools' compile.dart does, for the engine to run before `main`:
# it binds a plugin's Dart implementation for Linux (path_provider_linux,
# shared_preferences_linux) to its platform interface. None for an app
# with no plugins.
registrant="$app/.dart_tool/flutter_build/dart_plugin_registrant.dart"
plugins=()
if [[ -f "$registrant" ]]; then
  plugins=(--source "file://$registrant"
    --source package:flutter/src/dart_plugin_registrant.dart
    "-Dflutter.dart_plugin_registrant=file://$registrant")
fi

if [[ "$target" == kernel ]]; then
  sdk_root="$cache/artifacts/engine/common/flutter_patched_sdk"
  for f in "$dart" "$fes" "$sdk_root/platform_strong.dill"; do
    [[ -e "$f" ]] || { echo "build.sh: $f missing; run 'flutter precache --linux' at $flutter_root" >&2; exit 1; }
  done
  "$dart" "$fes" \
    --sdk-root "$sdk_root/" --target=flutter --no-print-incremental-dependencies \
    -Ddart.vm.profile=false -Ddart.vm.product=false \
    --link-platform \
    --packages "$app/.dart_tool/package_config.json" \
    "${plugins[@]}" \
    --output-dill "$out/$name.dill" \
    "$app/lib/main.dart" >/dev/null
  bundle "$out/$name.dill" app.dill
  echo "wrote $out/$name.dill and the bundle $out/$name"
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

sdk_root="$cache/artifacts/engine/common/flutter_patched_sdk_product"
gen_snapshot="$cache/artifacts/engine/$engine/gen_snapshot"
if [[ -n "${cross:-}" && ( "$(uname -m)" == x86_64 || -z "$engine" ) ]]; then
  [[ "$(uname -m)" == x86_64 ]] || { echo "build.sh: $target is cross-compiled on an x64 host only" >&2; exit 1; }
  # dartdev fetches its cross gen_snapshot on the first cross compile.
  sdk="$cache/dart-sdk"
  version="$(cat "$sdk/version")"
  gen_snapshot="$HOME/.dart/dartdev/sdk_cache/$version/gen_snapshot_linux_x64_linux_$cross"
  if [[ ! -x "$gen_snapshot" ]]; then
    echo 'void main() {}' > "$work/stub.dart"
    "$sdk/bin/dart" compile aot-snapshot --target-os linux --target-arch "$cross" \
      "$work/stub.dart" -o "$work/stub.aot" >/dev/null
  fi
  [[ -x "$gen_snapshot" ]] || { echo "build.sh: dartdev left no cross gen_snapshot at $gen_snapshot" >&2; exit 1; }
fi
for f in "$dart" "$fes" "$sdk_root/platform_strong.dill" "$gen_snapshot"; do
  [[ -e "$f" ]] || { echo "build.sh: $f missing; run 'flutter precache --linux' at $flutter_root" >&2; exit 1; }
done

"$dart" "$fes" \
  --sdk-root "$sdk_root/" --target=flutter --no-print-incremental-dependencies \
  -Ddart.vm.profile=false -Ddart.vm.product=true \
  --aot --tfa \
  --packages "$app/.dart_tool/package_config.json" \
  "${plugins[@]}" \
  --output-dill "$work/app.dill" \
  "$app/lib/main.dart"
"$gen_snapshot" --deterministic --snapshot_kind=app-aot-elf --elf="$out/$name.so" --strip "$work/app.dill"
bundle "$out/$name.so" libapp.so
echo "wrote $out/$name.so and the bundle $out/$name"
