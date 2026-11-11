#!/usr/bin/env bash
# Builds a Flutter app's libapp.so the way `flutter build linux --release`
# does, without the desktop embedding around it: frontend_server over the
# product platform (--aot --tfa), then the release gen_snapshot to an ELF.
#
#   build.sh <flutter-root> <app-dir> <out-dir> [target | kernel]
#
#   target: x86_64-unknown-linux-gnu (default) | aarch64-unknown-linux-gnu
#           (aarch64 cross-compiled on an x64 host: Flutter's linux-arm64
#           gen_snapshot is itself an arm64 binary, so the Dart SDK's
#           x64-to-arm64 one, the same VM revision, is used instead)
#   kernel: <out-dir>/<app>.dill instead, for fl-dart's JIT build: the
#           non-product platform, linked in, as `flutter run` compiles in
#           debug mode but in one file. Architecture-independent.
#
# Either way the app's asset bundle goes to <out-dir>/<app>.assets: what a
# Linux bundle keeps in data/flutter_assets (FontManifest.json, fonts, the
# app's own assets), from `flutter build bundle`. fl-dart run-app finds it
# beside the program.
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
  x86_64-unknown-linux-gnu) engine=linux-x64-release ;;
  aarch64-unknown-linux-gnu) engine=linux-arm64-release ;;
  kernel) engine= ;;
  *) echo "build.sh: unsupported target $target" >&2; exit 1 ;;
esac

cache="$flutter_root/bin/cache"
dart="$cache/dart-sdk/bin/dartaotruntime"
fes="$cache/dart-sdk/bin/snapshots/frontend_server_aot.dart.snapshot"
(cd "$app" && "$flutter_root/bin/flutter" pub get >/dev/null)

# The asset bundle, both modes. Release, so no kernel_blob.bin rides along.
rm -rf "$out/$name.assets"
(cd "$app" && "$flutter_root/bin/flutter" build bundle --release --no-pub \
  --target-platform linux-x64 --asset-dir "$out/$name.assets" >/dev/null)

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
    --output-dill "$out/$name.dill" \
    "$app/lib/main.dart" >/dev/null
  echo "wrote $out/$name.dill"
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

sdk_root="$cache/artifacts/engine/common/flutter_patched_sdk_product"
gen_snapshot="$cache/artifacts/engine/$engine/gen_snapshot"
if [[ "$target" == aarch64-unknown-linux-gnu && "$(uname -m)" == x86_64 ]]; then
  # dartdev fetches its cross gen_snapshot on the first cross compile.
  sdk="$cache/dart-sdk"
  version="$(cat "$sdk/version")"
  gen_snapshot="$HOME/.dart/dartdev/sdk_cache/$version/gen_snapshot_linux_x64_linux_arm64"
  if [[ ! -x "$gen_snapshot" ]]; then
    echo 'void main() {}' > "$work/stub.dart"
    "$sdk/bin/dart" compile aot-snapshot --target-os linux --target-arch arm64 \
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
  --output-dill "$work/app.dill" \
  "$app/lib/main.dart"
"$gen_snapshot" --deterministic --snapshot_kind=app-aot-elf --elf="$out/$name.so" --strip "$work/app.dill"
echo "wrote $out/$name.so"
