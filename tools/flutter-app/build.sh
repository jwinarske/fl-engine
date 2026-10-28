#!/usr/bin/env bash
# Builds a Flutter app's libapp.so the way `flutter build linux --release`
# does, without the desktop embedding around it: frontend_server over the
# product platform (--aot --tfa), then the release gen_snapshot to an ELF.
#
#   build.sh <flutter-root> <app-dir> <out-dir> [target | kernel]
#
#   target: x86_64-unknown-linux-gnu (default) | aarch64-unknown-linux-gnu
#   kernel: <out-dir>/<app>.dill instead, for fl-dart's JIT build: the
#           non-product platform, linked in, as `flutter run` compiles in
#           debug mode but in one file. Architecture-independent.
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
if [[ "$target" == kernel ]]; then
  sdk_root="$cache/artifacts/engine/common/flutter_patched_sdk"
  for f in "$dart" "$fes" "$sdk_root/platform_strong.dill"; do
    [[ -e "$f" ]] || { echo "build.sh: $f missing; run 'flutter precache --linux' at $flutter_root" >&2; exit 1; }
  done
  (cd "$app" && "$flutter_root/bin/flutter" pub get >/dev/null)
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

sdk_root="$cache/artifacts/engine/common/flutter_patched_sdk_product"
gen_snapshot="$cache/artifacts/engine/$engine/gen_snapshot"
for f in "$dart" "$fes" "$sdk_root/platform_strong.dill" "$gen_snapshot"; do
  [[ -e "$f" ]] || { echo "build.sh: $f missing; run 'flutter precache --linux' at $flutter_root" >&2; exit 1; }
done

(cd "$app" && "$flutter_root/bin/flutter" pub get >/dev/null)
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
"$dart" "$fes" \
  --sdk-root "$sdk_root/" --target=flutter --no-print-incremental-dependencies \
  -Ddart.vm.profile=false -Ddart.vm.product=true \
  --aot --tfa \
  --packages "$app/.dart_tool/package_config.json" \
  --output-dill "$work/app.dill" \
  "$app/lib/main.dart"
"$gen_snapshot" --deterministic --snapshot_kind=app-aot-elf --elf="$out/$name.so" --strip "$work/app.dill"
echo "wrote $out/$name.so"
