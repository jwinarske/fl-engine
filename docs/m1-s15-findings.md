# S15 — Dart VM embedding bring-up verification (Dart 3.13.7, rev 416e33b0ffaef062a60ec966bb8c82ed2b83f128)

Ground truth: `/home/user/src/dart-sdk` (git HEAD = 416e33b0ff…, `tools/VERSION` = 3.13.7 stable), prebuilt SDK
`/home/user/dl/dart-sdk-3.13.7/dart-sdk` (`revision` file = same hash), Flutter `stable` engine sources fetched from
raw.githubusercontent.com (Flutter stable `DEPS` pins `dart_revision: 416e33b0ff…` — the exact same Dart, so Flutter
code below is a valid reference). All paths below are relative to the Dart checkout unless prefixed `flutter:` (=
`engine/src/flutter/` in flutter/flutter@stable). Experiments live in
a scratch directory outside the repository.

## Headline API changes vs. older Dart (read first)

These invalidate a lot of "remembered" embedder code:

1. **There is no VM snapshot any more.** `Dart_InitializeParams` (runtime/include/dart_api.h:870-931,
   `DART_INITIALIZE_PARAMS_CURRENT_VERSION 0x0000000B` at :841) has **no `vm_snapshot_data` /
   `vm_snapshot_instructions` fields**. Fields: `version, create_group, initialize_isolate, shutdown_isolate,
   cleanup_isolate, cleanup_group, thread_start, thread_exit, file_open, file_read, file_write, file_close,
   entropy_source, get_service_assets (deprecated, no effect), start_kernel_isolate, code_observer`.
2. **`Dart_CreateIsolateGroup` takes `snapshot_data, snapshot_text`** (dart_api.h:1023-1030).
3. **AOT ELF symbols are `_kDartSnapshotData`, `_kDartSnapshotText`, `_kDartSnapshotBuildId` (and optional
   `_kDartSnapshotBss`)** — no more `_kDartVmSnapshotData/_kDartIsolateSnapshotData` (dart_api.h:4022-4037).
   Flutter's dart_snapshot.cc:18-21 maps both its "VM" and "isolate" symbol names onto `kDartSnapshotData/Text`.
4. Library target names use suffixes `_jit`, `_jit_product`, `_aotruntime`, `_aotruntime_product`, `_precompiler…`
   (runtime/configs.gni:78-209). The old names `libdart_precompiled_runtime`, `libdart_vm_precompiled_runtime`,
   `libdart_lib_precompiled_runtime`, `dart_precompiled_runtime` no longer exist.

---

## Q1. GN targets, output names, what the executables link

### Config matrix (runtime/configs.gni:78-209)
`library_for_all_configs(name)` (configs.gni:210-283) instantiates `name + suffix` for each entry of `_all_configs`:

| suffix | configs | defines (effective) | compiler lib linked? |
|---|---|---|---|
| `_jit` | base + `dart_maybe_product_config` | PRODUCT only if `dart_runtime_mode=="release"` | yes |
| `_jit_product` | base + `dart_product_config` | PRODUCT if `!dart_debug` | yes |
| `_aotruntime` | base + maybe_product + `dart_aotruntime_config` | `DART_PRECOMPILED_RUNTIME`, `EXCLUDE_CFE_AND_KERNEL_PLATFORM` (+PRODUCT iff runtime_mode release) | no |
| `_aotruntime_product` | base + aotruntime + `dart_product_config` | same + PRODUCT if `!dart_debug` | no |
| `_precompiler`, `_precompiler_product`, `_precompiler_product_linux_{x64,arm64,arm,riscv64}`, `_host_targeting_host`… | precompiler config (`DART_PRECOMPILER`) | gen_snapshot flavours | yes |

`dart_maybe_product_config` / `dart_product_config` / `dart_aotruntime_config`: runtime/BUILD.gn:19-59
(release mode also adds `DART_DISABLE_TIMELINE` on Linux, :33-36 / :47-49).

### VM proper (all in runtime/, none need dart:io)
| GN label | type | contents |
|---|---|---|
| `//runtime:libdart{suffix}` (BUILD.gn:364-400) | `dart_component_kind` = **static_library** by default (runtime_args.gni) → `out/<cfg>/obj/runtime/libdart_jit.a`, `libdart_aotruntime_product.a`, … | `dart_api_impl.cc`, `native_api_impl.cc`, `analyze_snapshot_api_impl.cc`, generated `version.cc`; deps below |
| `//runtime/vm:libdart_vm{suffix}` (vm/BUILD.gn:72-118) | **source_set** | vm_sources + compiler API + disassembler + ffi + heap + regexp; deps `//third_party/icu:icuuc`, `//third_party/perfetto:libprotozero` (if `dart_support_perfetto`), BoringSSL only for precompiler configs (Mach-O SHA256) |
| `//runtime/vm:libdart_lib{suffix}` (vm/BUILD.gn:136-155) | source_set | runtime/lib/*.cc natives for core/async/isolate/ffi/typed_data/mirrors/developer/vmservice + bootstrap.cc |
| `//runtime/vm:libdart_compiler{suffix}` (vm/BUILD.gn:120-134) | source_set | JIT/AOT compiler; only pulled in for configs with `compiler=true` (jit, precompiler) |
| `//runtime/platform:libdart_platform{suffix}` (platform/BUILD.gn:8-21) | source_set | platform/ utils |
| `//third_party/double-conversion/src:libdouble_conversion` | source_set (vendored in-tree, third_party/double-conversion/src/BUILD.gn) | |
| `//runtime:dart_jit_library` (BUILD.gn:402-408) | shared_library `libdart_jit.so` | wraps `:libdart_jit` |

Because vm/lib/platform/double-conversion are source_sets, GN folds their objects into `libdart_<suffix>.a`.
**Not** folded (separate static libs you must also link): ICU `icuuc` (and Perfetto protozero objects if enabled —
protozero is a source_set too, so it will be absorbed; ICU is a static_library in Chromium's icu BUILD.gn).
System libs: `dl`, `pthread` (vm/BUILD.gn:45-70 `libdart_vm_config`; `atomic` only for non-clang).

### Embedder support (runtime/bin) — this is where dart:io lives
| GN label | notes |
|---|---|
| `//runtime/bin:libdart_builtin{,_product,…}` (bin/BUILD.gn:53-127) | source_set, builtin_impl_sources (file/OS helpers incl. `bin::GetEntropy`, `OpenFile` etc.) |
| `//runtime/bin:common_embedder_dart_io{,_product}` (bin/BUILD.gn:529-615) | source_set: dart:io natives + `builtin.cc` + `dart_io_api_impl.cc`; **deps `//third_party/zlib` and `//third_party/boringssl`** |
| `//runtime/bin:native_assets_api{,_product}`, `shared_object_loaders`, `elf_loader` (bin/BUILD.gn:129-247) | helpers |
| `//runtime/bin:core_snapshot_bin` (bin/BUILD.gn:617-635) | runs gen_snapshot `--snapshot_kind=core` on `vm_platform_stripped.dill` → `core_snapshot_{data,text}.bin`, linked as symbols `kDartCoreSnapshotData/Text` (bin/BUILD.gn:745-757) |
| `//runtime/bin:dart_embedder_runtime_jit` (bin/BUILD.gn:1274-1315) | `complete_static_lib` bundling libdart_jit + dart:io + kernel/platform dill |
| `//runtime/engine:dart_engine_{jit,aot}_{static,shared}` (runtime/engine/BUILD.gn) | new "Dart Engine" embedding API (runtime/engine/README.md); also drags dart:io + builtin |

### What the executables link (bin/BUILD.gn)
`dart_executable` template (bin/BUILD.gn:815-938): every executable gets `crashpad`, `icudtl_cc` (embedded
`icudtl.dat` as `kIcuData`), `//third_party/boringssl`, `//third_party/icu:icuuc`, `//third_party/zlib`,
`common_embedder_dart_io[_product]`, and config `export_api_symbols` (`-Wl,--export-dynamic-symbol=Dart_*`, :36-51).

- **`dartaotruntime`** (bin/BUILD.gn:969-1003) = old `dart_precompiled_runtime`: `..:libdart_aotruntime`,
  `../platform:libdart_platform_aotruntime`, `icudtl_cc`, `native_assets_api`, `shared_object_loaders`
  (+ `_product` variants when `dart_runtime_mode=="release"`), sources main.cc/main_impl.cc/loader.cc/
  `snapshot_empty.cc` (defines `kDartCoreSnapshotData/Text = nullptr`).
- **`dartaotruntime_product`** (:1005-1033): `..:libdart_aotruntime_product` etc. — this is what the SDK ships as
  `bin/dartaotruntime` (tools/gn.py:277 strips `dartaotruntime_product`); its snapshots say `product` (see Q7).
- **`dartvm`** (JIT host, :940-967): `..:libdart_jit`, `../platform:libdart_platform_jit`, `:core_snapshot_cc`,
  `:dart_kernel_platform_cc` (kernel-service dill + `vm_platform.dill` as `kPlatformDill`), `native_assets_api`,
  `dfe.cc`.
- **`dart`** (:1035-1061): dartdev launcher on `libdart_aotruntime_product`.
- **`gen_snapshot`** (:249-345): `..:libdart_precompiler`, `libdart_platform_precompiler`, `libdart_builtin`,
  `gen_snapshot_dart_io` (dart:io natives, `DART_IO_SECURE_SOCKET_DISABLED`), zlib. Cross variants:
  `gen_snapshot_product_linux_{x64,arm64,arm,riscv64}` (:361-411).

**To embed without dart:io: link only `libdart_jit.a` (or `libdart_aotruntime_product.a`) + `icuuc` (+ dl,
pthread).** Nothing in `//runtime:libdart*` depends on runtime/bin. dart:io *Dart* code is still inside
`vm_platform.dill`, but its natives are just unresolved until called.

### GN args for release/product
- `build/config/BUILDCONFIG.gn:108-117`: `is_debug=true` by default; `is_release`, `is_product` default false;
  :343-351 asserts exactly one of debug/release/product → **`is_debug=false` alone fails the assert; you must also set
  `is_release=true` or `is_product=true`.**
- `tools/gn.py:239-250` (canonical): `is_debug = mode=="debug"`, `is_release = mode=="release"`,
  `is_product = mode=="product"`, `dart_debug = mode=="debug"`, `dart_runtime_mode = "release"` only for
  `mode=="product"`, else `"develop"`. Also `dart_use_compressed_pointers` only for `*64c` archs (:147, :210).
- `dart_runtime_mode` (runtime_args.gni) only toggles whether the *unsuffixed* libs (`libdart_jit`,
  `libdart_aotruntime`) get `PRODUCT`; the TODO there says it no longer selects JIT vs AOT. **AOT vs JIT is chosen
  by the target (`_aotruntime` suffix), not by args.** `dart_runtime_mode="release"` also disables Perfetto
  (`dart_support_perfetto` default, runtime_args.gni).
- Standalone-Dart way to get a product AOT runtime: `is_debug=false is_release=true` (or `is_product=true`) and build
  `//runtime:libdart_aotruntime_product` (PRODUCT because `!dart_debug`). Flutter way: `is_debug=false is_product=true
  dart_runtime_mode="release"` and use `libdart_aotruntime`/`libdart_jit` (now PRODUCT).
- **Snapshot compatibility constraint:** SDK-prebuilt gen_snapshot emits `product … no-compressed-pointers` AOT
  snapshots (Q7) ⇒ pair them only with a **PRODUCT, non-compressed-pointer** AOT runtime
  (`libdart_aotruntime_product`, `dart_use_compressed_pointers=false`, which is the default).

## Q2. Is `dart compile kernel` output directly consumable?

- `dart compile kernel` → `--link-platform` **defaults to true** (pkg/dartdev/lib/src/commands/compile.dart:261-267,
  passed at :346). Verified: `hello.dill` = 8,621,160 bytes (contains the whole VM platform); with
  `--no-link-platform` = 624 bytes. `pkg/vm/lib/kernel_front_end.dart:121-125` (gen_kernel) also defaults true;
  `pkg/frontend_server/lib/frontend_server.dart:119-128` defaults true, help text: "gen_snapshot (which needs platform
  embedded) vs Flutter engine (which does not)".
- `Dart_CreateIsolateGroupFromKernel(kernel_buffer)` bootstraps **dart:core etc. from that buffer**:
  dart_api_impl.cc:1320-1347 → `Dart::InitializeIsolateGroup` → `InitIsolateGroupFromSnapshot` →
  `Object::Init(IG, kernel_buffer, size)` (dart.cc:673-760, object.cc:1654-1670) → `Bootstrap::DoBootstrapping`
  (bootstrap.cc:185-210) → loads bootstrap libs then `loader.LoadProgram()` for every other library in the dill
  (bootstrap.cc:78-121). So the buffer **must contain the platform** — a `--no-link-platform` dill cannot be the
  group-creation buffer.
- The root library is *not* set by group creation. `Dart_LoadScriptFromKernel` (dart_api_impl.cc:5539-5594) loads a
  (possibly platform-less) app dill, picks the library with `main`, and sets the root library; it errors if a root
  library is already set.
- What `dartvm` itself does (runtime/bin/main_impl.cc:658-745, 286-337): JIT normally uses the embedded **core
  snapshot** `Dart_CreateIsolateGroup(core_snapshot_data, core_snapshot_text)`; if there is no core snapshot it uses
  `dfe.LoadPlatform()` (vm_platform.dill) — or, if no platform is linked in, **the app kernel itself** — with
  `Dart_CreateIsolateGroupFromKernel`, then always `Dart_LoadScriptFromKernel(app_kernel)`. Verified: prebuilt
  `bin/dartvm hello.dill` and `bin/dartvm hello_nolink.dill` both print `hello`.

**Two working bare-embedder recipes (JIT):**
```c
// A: platform + thin app dill (mirrors dartvm fallback, main_impl.cc:710-731 then :336)
iso = Dart_CreateIsolateGroupFromKernel(uri, "main", platform_dill /*SDK lib/_internal/vm_platform.dill*/,
                                        platform_len, &flags, group_data, iso_data, &err);
Dart_EnterScope();
Dart_Handle root = Dart_LoadScriptFromKernel(app_nolink_dill, app_len);   // sets root lib
// B: one self-contained dill (default `dart compile kernel` output) used for both calls
iso = Dart_CreateIsolateGroupFromKernel(uri, "main", app_dill, app_len, ...);
Dart_EnterScope();
Dart_Handle root = Dart_LoadScriptFromKernel(app_dill, app_len);
```
Buffers must stay alive until group shutdown. Recipe A with the SDK's own `vm_platform.dill` is safe because the SDK
is the same revision (Q8). (I could not run these against a self-built libdart — none exists in the container — so
they are derived from the code paths above, which `dartvm` exercises.)

**Flutter (flutter:runtime/dart_isolate.cc):** root isolate is created with `Dart_CreateIsolateGroup(isolate
snapshot data, instructions)` (:263-270), i.e. from a *core snapshot*, never from the platform dill. Then
`PrepareForRunningFromKernel` (:756-790) does `Dart_SetRootLibrary(Dart_Null())` and `LoadKernel` (:727-750):
`Dart_LoadLibraryFromKernel(piece)` for each piece, `Dart_SetRootLibrary(last)`, `Dart_FinalizeLoading(false)`.
The app dill Flutter feeds it is built **without** the platform (frontend_server `--no-link-platform`).
`flutter_tester` (flutter:shell/testing/tester_main.cc:359-383, :582) just takes `application_kernel_asset` and goes
through the same path.

## Q3. Which snapshot blobs are needed; can JIT run snapshot-less?

- `Dart_Initialize` needs **no snapshot at all** at this revision (no fields for it; dart.cc:274-367 `DartInit`).
- `Dart_CreateIsolateGroup(…, snapshot_data=NULL, snapshot_text=NULL, …)` → error **"Missing isolate snapshot"**
  (dart.cc:735-738). JIT given an AOT snapshot → "JIT runtime cannot run an AOT snapshot" (:698-701); AOT runtime
  given JIT → "AOT runtime cannot run a JIT snapshot" (:694-697).
- **Kernel-only JIT works**: `Dart_CreateIsolateGroupFromKernel` with a platform-containing dill (Q2). The
  `skip_bootstrap=false` path runs `Object::Init`, `StubCode::Init`, `Symbols::Init` etc. at isolate-group creation
  (dart.cc:768-812), which is slower (bootstraps the core libs from kernel each group) but needs nothing else. There is
  no `dart_no_snapshot`/`libdart_nosnapshot` target any more (grep finds none) — `libdart_jit` simply works with or
  without a core snapshot.
- Optional JIT speed-up: a **core snapshot** — `gen_snapshot --snapshot_kind=core --snapshot_data=… --snapshot_text=…
  vm_platform_stripped.dill` (bin/BUILD.gn:617-635; gen_snapshot.cc:161-165, :261-270). Emitted by
  `//runtime/bin:core_snapshot_bin` into `$target_out_dir/core_snapshot_{data,text}.bin`, then linked as
  `kDartCoreSnapshotData/Text` (bin/BUILD.gn:745-757, main_impl.cc:44-53). Verified the prebuilt SDK gen_snapshot can
  produce one (9.7 MB data, 0-byte text) **but its header says `product`** — core snapshots carry the build-mode
  feature string (`debug|release|product`, dart.cc:909-920), so a prebuilt-gen_snapshot core snapshot only loads in a
  PRODUCT libdart_jit. For a non-product (debuggable) libdart_jit, build your own `gen_snapshot` (non-product
  `//runtime/bin:gen_snapshot`) or just use the kernel path.
- **Flutter debug (JIT)**: passes `isolate_snapshot_data` + `isolate_snapshot_instr` (core snapshot produced by the
  engine's gen_snapshot, shipped in flutter_assets) to `Dart_CreateIsolateGroup`; the VM-snapshot settings
  (`vm_snapshot_data`) still exist in Flutter's Settings but resolve to the same `kDartSnapshotData` symbol and are not
  passed to `Dart_Initialize` (flutter:runtime/dart_snapshot.cc:18-21, :102-175; flutter:runtime/dart_vm.cc:445-470 —
  `params` has no snapshot fields).

## Q4. Getting `print()` output in a bare embedder

Chain: `print()` (sdk/lib/core/print.dart:18-26) → `printToConsole` (external in sdk/lib/internal/print.dart:16) →
VM patch sdk/lib/_internal/vm/lib/print_patch.dart:11-24:
```dart
@patch void printToConsole(String line) { _printClosure(line); }
void _unsupportedPrint(String line) { throw UnsupportedError("'print' is not supported"); }
@pragma("vm:entry-point", "set") @pragma("vm:shared")
_PrintClosure _printClosure = _unsupportedPrint;      // in library dart:_internal
```
So **without setup, `print` throws `UnsupportedError("'print' is not supported")`** (also hit by unhandled async
error reporting).

- **runtime/bin**: `DartUtils::PrepareBuiltinLibrary` (runtime/bin/dartutils.cc:455-466):
  `print = Dart_Invoke(dart:_builtin, "_getPrintClosure")`; `Dart_SetField(dart:_internal, "_printClosure", print)`.
  `_getPrintClosure`/`_print` live in sdk/lib/_internal/vm/bin/builtin.dart:24-35 and call native
  `Builtin_PrintString` (runtime/bin/builtin_natives.cc:28, :89-…), resolved by the bin native resolver.
- **Flutter**: flutter:lib/ui/dart_runtime_hooks.cc:49-56 does the same with `dart:ui`'s `_getPrintClosure`
  (flutter:lib/ui/natives.dart:113-118, `@pragma('vm:entry-point')`), whose `_print` calls
  `@Native<Void Function(Handle)>(symbol: 'DartRuntimeHooks::Logger_PrintString')` (natives.dart:25-39) →
  `Logger_PrintString` (dart_runtime_hooks.cc:140-160). `PrintDebug` = `_printDebug`/`Logger_PrintDebugString` (only
  in !NDEBUG).
- The VM's own test does it with a user closure: runtime/vm/isolate_test.cc:52-61.

**Exact sequence for an embedder without dart:io** (after the root library is loaded, inside a scope):
```dart
// in the app (or a host-provided library compiled into the dill):
import 'dart:ffi';
@Native<Void Function(Pointer<Utf8>)>(symbol: 'host_print')  // or Handle-based, see below
external void _hostPrint(...);
@pragma('vm:entry-point')            // required: Dart_GetField checks entry points (FLAG_verify_entry_points=true, flag_list.h:261)
void _embedderPrint(String line) { /* call native */ }
```
```c
Dart_Handle internal = Dart_LookupLibrary(Dart_NewStringFromCString("dart:_internal"));
Dart_Handle closure  = Dart_GetField(root_lib, Dart_NewStringFromCString("_embedderPrint")); // tear-off: Library::InvokeGetter creates ImplicitStaticClosure (object.cc InvokeGetter)
Dart_Handle r        = Dart_SetField(internal, Dart_NewStringFromCString("_printClosure"), closure);
```
Simplest native for the closure: `@Native<Void Function(Handle)>(symbol: 'host_print') external void
_hostPrint(String s);` with a host `void host_print(Dart_Handle s)` that does `Dart_StringToUTF8` — this is exactly
Flutter's pattern. Alternative with zero Dart helper code: install a classic native resolver on `dart:_builtin`
(`Dart_SetNativeResolver(dart:_builtin, …)` returning your `Builtin_PrintString`) and then reuse
`dart:_builtin::_getPrintClosure` exactly like runtime/bin (dart:_builtin is in vm_platform.dill).

**Also required for anything async** (same file family; without these `scheduleMicrotask`/`await` throws
"Microtasks are not supported", schedule_microtask_patch.dart:66-74):
```c
Dart_Handle isolate_lib = Dart_LookupLibrary(NewString("dart:isolate"));
Dart_Handle sched = Dart_Invoke(isolate_lib, NewString("_getIsolateScheduleImmediateClosure"), 0, NULL);
Dart_Invoke(Dart_LookupLibrary(NewString("dart:async")), NewString("_setScheduleImmediateClosure"), 1, &sched);
Dart_Invoke(isolate_lib, NewString("_setupHooks"), 0, NULL);   // sets VMLibraryHooks.timerFactory (timer_impl.dart:513-516)
```
(dartutils.cc:493-526; Flutter dart_runtime_hooks.cc:65-69, :82-99.) Caveat: VM `Timer`s post to
`VMLibraryHooks.eventHandlerSendData`, which is only assigned by dart:io's `_setupHooks`
(sdk/lib/_internal/vm/bin/common_patch.dart:65; timer_impl.dart:465-472) — **so `Timer`/`Future.delayed` need either
dart:io's event handler or an embedder replacement for `VMLibraryHooks.timerFactory`** (Flutter is fine because it
links dart:io). `Uri.base` needs `dart:core::_uriBaseClosure` (dartutils.cc:470-479), optional.

**Dart_InitializeParams callbacks** (dart_api.h:870-931; consumed in dart.cc:302-306, :358-362):
- `create_group`: needed only for `Isolate.spawn*`/service isolate; NULL ⇒ spawning fails.
  `initialize_isolate` needed for `Isolate.spawn` in-group isolates; shutdown/cleanup optional.
- `entropy_source`: NULL ⇒ `Random()` seeds from `OS::GetCurrentTimeMicros()` (vm/random.cc:17-32), and
  **`Random.secure()` throws UnsupportedError "No source of cryptographically secure random numbers available."**
  (runtime/lib/math.cc:20-32). Provide it (getrandom()).
- `file_open/read/write/close`: NULL is fine; only used for diagnostics outputs (timeline file recorder, perf map
  `os_linux.cc:84`, `--write_v8_snapshot_profile_to`, heap snapshots, service, precompiler traces). Each call site
  null-checks.
- `thread_start/thread_exit`: optional. `start_kernel_isolate`: leave false (no CFE in a bare embedder).
- `version` must equal `DART_INITIALIZE_PARAMS_CURRENT_VERSION` or `Dart_Initialize` returns "Invalid
  Dart_InitializeParams version." (dart_api_impl.cc:1125-1139). `Dart_SetVMFlags` must have been called (even with
  argc=0) — `DartInit` returns "VM initialization failed-VM Flags not initialized." otherwise (dart.cc:291-293).
- Flutter sets `create_group, initialize_isolate, shutdown_isolate, cleanup_isolate, cleanup_group, thread_exit,
  file_*` (=`dart::bin::*`) and `entropy_source = dart::bin::GetEntropy` (flutter:runtime/dart_vm.cc:446-465).

ICU note: libdart_vm links ICU (`//third_party/icu:icuuc`, used by the irregexp port, vm/regexp/*.cc with
`V8_INTL_SUPPORT` hard-defined in vm/regexp/base.h:50). The dart executables additionally embed `icudtl.dat` and
call `udata_setCommonData` (runtime/bin/icu.cc:12-26). Whether a bare embedder must load ICU data for regexp
(case-folding / `\p{…}` property classes) is **uncertain** — basic `uchar.h` properties are compiled into icuuc,
UnicodeSet property/locale lookups may need data. Safest: call `udata_setCommonData` with `icudtl.dat` (or link a
system ICU that has its data library).

## Q5. DynamicLibrary.process() / @Native symbol resolution on Linux

- `DynamicLibrary.process()` = `Ffi_dl_processLibrary` → `DynamicLibrary::New(RTLD_DEFAULT, …)`
  (runtime/lib/ffi_dynamic_library.cc:215-225); lookups go to `Utils::ResolveSymbolInDynamicLibrary` =
  `dlsym(handle, symbol)` (runtime/platform/utils.cc:367-377). **So yes: `dlsym(RTLD_DEFAULT, …)`** on
  Linux/Android/macOS/Fuchsia for every arch (x64/arm64/riscv64 identical).
- `DynamicLibrary.executable()` = `dlopen(NULL)` (`LoadDynamicLibrary(nullptr)`, :227-230).
- Consequence: symbols in the **host executable** are only visible if they are in its **dynamic symbol table**.
  Rust: `#[no_mangle] pub extern "C" fn …` **plus** link with `-C link-arg=-Wl,--export-dynamic` (≡ `-rdynamic`), or
  selectively `-C link-arg=-Wl,--export-dynamic-symbol=host_*` (lld/GNU ld ≥2.35; this is what Dart itself uses for
  `Dart_*`, bin/BUILD.gn:36-51) or `-Wl,--dynamic-list=list.txt`. Make sure the functions are not dead-stripped
  (referenced or `#[used]`); verify with `nm -D --defined-only <bin> | grep host_`. (I did not build a Rust binary
  here; flag semantics are standard ld behavior.) Shared libraries already loaded with RTLD_GLOBAL (incl. libc) are
  also found.
- **@Native without asset** — resolution order in `FfiResolveInternal` (ffi_dynamic_library.cc:563-610):
  1. `Dart_SetFfiNativeResolver(library, resolver)` registered on the library whose URI equals the asset id
     (default asset id = the declaring library's URI; `GetFfiNativeResolver` :300-309),
  2. native-assets mapping / embedder `NativeAssetsApi` (`Dart_InitializeNativeAssetsResolver`), :504-516 and
     :462-500 — with no NativeAssetsApi and no mapping this returns "not found, no error",
  3. **`dlsym(RTLD_DEFAULT, symbol)`** (:582-588).
  Documented in sdk/lib/ffi/ffi.dart:2683-2691. Note: if an embedder installs `NativeAssetsApi::dlopen_v2`, unknown
  assets become errors and step 3 is skipped (:469-478).
- **Empirical check** (prebuilt AOT runtime): `@Native<Int32 Function()>() external int getpid();` resolves via process
  lookup and prints the pid; `DynamicLibrary.process().providesSymbol("Dart_PostInteger")` → true (dartaotruntime
  exports 318 `Dart_*` via `--export-dynamic-symbol`); a missing symbol yields `Couldn't resolve native function 'X' in
  'file:///…ffi_t.dart' : No asset with id '…' found. No available native assets. Attempted to fallback to process
  lookup. …: undefined symbol: X.`
- **M2 FFI-first verdict: feasible.** Best option avoids symbol export entirely: register
  `Dart_SetFfiNativeResolver(root_or_binding_lib, resolver)` where `resolver(name, nargs)` returns Rust fn pointers
  from a table — works for JIT and AOT, no `-rdynamic`, no dlsym. Fallback: export symbols and rely on step 3.
  `Handle`-typed params (`Dart_Handle`) are allowed in @Native (Flutter uses it for print).

## Q6. Dart_IsolateFlags at this version

`DART_FLAGS_CURRENT_VERSION 0x0000000e`; struct (dart_api.h:581-600):
`version, enable_asserts, use_field_guards, use_osr, obfuscate, load_vmservice_library, null_safety,
is_system_isolate, is_service_isolate, is_kernel_isolate, snapshot_is_dontneed_safe, branch_coverage, coverage,
dwarf_stack_traces, code_comments`.

- Always initialize with `Dart_IsolateFlagsInitialize(&flags)` (dart_api_impl.cc:1284-1286 → isolate.cc:1730-1741);
  passing `flags=NULL` to the create calls does the same (dart_api_impl.cc:1298-1302, :1330-1334). `version` is
  **not validated** anywhere (only written, isolate.cc:1683/1694/1733/1746).
- `null_safety`: still a field, but **ignored** — FlagsInitialize/FlagsCopyTo force it to `true`
  (isolate.cc:1690, :1701, :1740, :1753); nothing reads it.
- Mapping to VM state: isolate.h:136-159 (`BOOL_ISOLATE_GROUP_FLAG_LIST` / `BOOL_ISOLATE_FLAG_LIST`). Defaults come
  from command-line flags (`--enable-asserts`, `--use-osr`, …); `obfuscate` honored only in precompiler;
  `enable_asserts`, `snapshot_is_dontneed_safe`, coverage, `is_system/service/kernel_isolate` are **NONPRODUCT-only**
  (ignored in PRODUCT builds). For AOT snapshots the group flags are overwritten from the snapshot header
  (`SnapshotHeaderReader::InitializeIsolateGroupFlagsFromSnapshot`, dart.cc:775-783).
- Things a root app isolate should leave at defaults: `is_system_isolate=false`, `is_service_isolate=false`,
  `is_kernel_isolate=false`, `load_vmservice_library=false`. Set `snapshot_is_dontneed_safe=true` only if the AOT
  snapshot pages are file-backed mappings (dlopen'ed ELF / mmap of file) that can be `MADV_DONTNEED`'d.
- **Flutter root isolate** (flutter:runtime/dart_isolate.cc:62-84, :112-114): starts from
  `Dart_IsolateFlagsInitialize` (or the VM-provided flags for spawns), then sets only
  `null_safety = isolate_configuration->IsNullSafetyEnabled(snapshot)` (no-op now) and
  `snapshot_is_dontneed_safe = isolate_snapshot->IsDontNeedSafe()`. Service isolate additionally gets
  `load_vmservice_library = true` (:991) and `null_safety` (:997). Asserts come from VM flags
  (`--enable-asserts` in debug, flutter:runtime/dart_vm.cc).

## Q7. AOT cross-compilation and the ELF output

- `dart compile aot-snapshot`/`exe` accept `--target-os` and `--target-arch` (compile.dart:606-616); supported
  targets `linux-arm`, `linux-arm64`, `linux-riscv64`, `linux-x64` (compile.dart:1171-1176). Kernel is always built
  with the host `dartaotruntime` + `gen_kernel_aot.dart.snapshot` (:648-655).
- The x64 SDK **does not ship cross gen_snapshots**: `resolveTargetBinaries` (:1210-1300) first looks for
  `gen_snapshot_product_<os>_<arch>` next to a `build.ninja`, otherwise `SdkCache.ensureGenSnapshot` downloads
  `gen_snapshot_<host>_<target>` and `dartaotruntime_<target>` (pkg/dartdev/lib/src/sdk_cache.dart:172-205) from
  `https://storage.googleapis.com/dart-archive/channels/<channel>/raw/hash/<revision>/sdk/<file>`
  (sdk_cache.dart:28-33, :306-313) into `~/.dart/dartdev/sdk_cache/<version>/`.
  **Verified working in this container** (storage.googleapis.com is reachable): both
  `--target-arch=arm64` and `--target-arch=riscv64` succeeded and cached
  `gen_snapshot_linux_x64_linux_{arm64,riscv64}` (~4.7 MB each, x86-64 executables) and
  `dartaotruntime_linux_{arm64,riscv64}` in `/root/.dart/dartdev/sdk_cache/3.13.7/`. The same files are also at
  `…/channels/stable/release/3.13.7/sdk/gen_snapshot_linux_x64_linux_arm64` (HTTP 200). A pipeline can either call
  `dart compile aot-snapshot --target-arch=…` or run those gen_snapshots directly
  (`gen_snapshot --snapshot_kind=app-aot-elf --elf=out.so app_aot.dill`, gen_snapshot.cc:177-183; the input dill must
  be AOT-mode kernel from `gen_kernel --aot`, which dartdev does for you).
- Output ELF (`readelf`): `ELF64 DYN (Shared object)`, x86-64 / AArch64 / RISC-V RVC double-float, `.dynsym` =
  ```
  _kDartSnapshotText    OBJECT GLOBAL  .text   (R E)
  _kDartSnapshotData    OBJECT GLOBAL  .rodata (R)
  _kDartSnapshotBuildId OBJECT GLOBAL  .note.gnu.build-id
  ```
  no `_kDartSnapshotBss`, no DT_NEEDED, no relocations; LOAD alignment 0x4000 (x64, riscv64) / 0x10000 (arm64).
- **dlopen-able by glibc: yes** — a small C program `dlopen(RTLD_NOW|RTLD_LOCAL)` + `dlsym` returned all three
  symbols (`_kDartSnapshotBss` = NULL). Snapshot data header: magic `f5f5dcdc`, then the 32-hex snapshot hash
  `0451907c2eaa8467e848c0067bfe8ed4` and feature string
  `product no-asan no-msan no-tsan no-shared_data no-code_comments no-dwarf_stack_traces arm64 linux no-compressed-pointers`.
  Pass `_kDartSnapshotData`/`_kDartSnapshotText` to `Dart_CreateIsolateGroup` (dart_api.h:4045-4050). Alternatively
  runtime/bin `elf_loader` (bin/BUILD.gn:183-213, bin/elf_loader.h:22) loads it without dlopen.
  AOT runtime on Linux also checks page size ≤ ELF page size (dart.cc:314-321) — arm64 snapshots use 64K alignment
  so 4K/16K/64K kernels are all fine.

## Q8. Version strings and mismatch detection

- `Dart_VersionString()` = `Version::String()` (dart_api_impl.cc:1121-1123), template vm/version_in.cc:
  `"{{VERSION_STR}} ({{CHANNEL}}) ({{COMMIT_TIME}}) on \"<hostos>_[sim]<arch>\""` → e.g.
  `3.13.7 (stable) (Thu Oct 22 14:28:57 2026 -0700) on "linux_x64"` (matches `dart --version`). Without git info
  (`dart_version_git_info=false`) COMMIT_TIME = "Unknown timestamp" (tools/make_version.py:74-79).
- **Snapshot hash** `Version::SnapshotString()` = md5 over 15 VM source files (tools/make_version.py:20-45:
  app_snapshot.{cc,h}, dart.cc, dart_api_impl.cc, datastream.h, image_snapshot.{cc,h}, object.{cc,h},
  raw_object.{cc,h}, snapshot.{cc,h}, symbols.{cc,h}). **Not exported via any Dart_* API** (only `Dart_VersionString`,
  dart_api.h:571). Recomputed from the checkout: `0451907c2eaa8467e848c0067bfe8ed4` — **identical** to the string in
  the prebuilt `dartaotruntime`, `dartvm`, `gen_snapshot` and in the generated AOT snapshots ⇒ a libdart built from
  this checkout accepts SDK-generated snapshots (given matching features). The embedder can read it from snapshot
  bytes (offset 20, 32 chars) for its own pre-check.
- Errors (app_snapshot.cc:9732-9792, surfaced through `Dart_CreateIsolateGroup`'s `*error`):
  `"No full snapshot version found, expected '%s'"`, `"Wrong full snapshot version, expected '%s' found '%s'"`
  (the format is `"Wrong %s snapshot version, …"` with `full|script`), and
  `"Snapshot not compatible with the current VM configuration: the snapshot requires '%s' but the VM has '%s'"`
  (features: debug/release/product, sanitizers, arch, OS, compressed-pointers; dart.cc:909-1005).
  Also "Invalid snapshot" (dart.cc:690-693).
- **Kernel header** (pkg/kernel/binary.md:149-151): `UInt32 magic = 0x90ABCDEF; UInt32 formatVersion = 138;
  Byte[10] shortSdkHash;`. Verified `hello.dill` starts `90 ab cd ef 00 00 00 8a "416e33b0ff"`. VM side
  (vm/kernel_binary.cc:66-136, kernel_binary.h:21 `kSupportedKernelFormatVersion = 138`): errors
  `"Invalid magic identifier"`, `"Invalid kernel binary format version (expected 138, found N)"`,
  `"Invalid SDK hash"`, `"File size is too small to be a valid kernel file"`, wrapped as
  `"Can't load Kernel binary: <msg>."` (bootstrap.cc:133-137, dart_api_impl.cc:5567). The SDK-hash check is skipped
  if either side's hash is `"0000000000"` (kernel_binary.cc:80-87); the VM's hash is `git rev-parse --short=10 HEAD`
  (tools/utils.py:440-452) unless `verify_sdk_hash=false` (sdk_args.gni) or no git ⇒ `0000000000`. **When building
  libdart from the shallow checkout, git works (HEAD=416e33b0ff), so the hash will match SDK-produced dills; if you
  build from a tarball, pass `dart_sdk_verification_hash="416e33b0ff"` or `verify_sdk_hash=false`.**
  `Dart_IsKernel(buf,len)` (dart_api.h:1428) checks the magic only.

## Q9. Building libdart in this container without googlesource

### Network reachability (probed via the agent proxy)
| host | result |
|---|---|
| chrome-infra-packages.appspot.com | **blocked** (`CONNECT tunnel failed, response 403`) — no CIPD: no buildtools clang, gn, ninja, sysroot, checked-in Dart SDK |
| chromium.googlesource.com, dart.googlesource.com, gn.googlesource.com | blocked (403 on CONNECT, proxy status log) |
| github.com via **git** (`git ls-remote/fetch`) | works |
| github.com web / api.github.com / codeload | 403 (only git transport and raw.githubusercontent.com work) |
| raw.githubusercontent.com | works (also at pinned commits) |
| storage.googleapis.com (dart-archive) | works |
| archive.ubuntu.com | works (apt pool browsable) |

### DEPS entries needed for `//runtime:libdart_jit` / `libdart_aotruntime_product` (+ optional gen_snapshot)
| path (DEPS) | pin | needed for | github mirror |
|---|---|---|---|
| third_party/icu (chromium/deps/icu, DEPS:322-324, `icu_rev a86a32e6…`) | | **libdart_vm (mandatory)** | **no mirror of Chromium's fork found** (`chromium/icu`, `flutter/icu` don't exist). Upstream `github.com/unicode-org/icu` exists (no Chromium BUILD.gn). Workaround: provide your own `//third_party/icu/BUILD.gn` with `icuuc` (source_set over unicode-org icu4c/source/common, or a config linking system `libicuuc` from Ubuntu's libicu-dev) |
| third_party/perfetto/src (`perfetto_rev 13ce0c9e…`) | | only if `dart_support_perfetto` (default true unless `dart_runtime_mode="release"`); can set `dart_support_perfetto=false` | yes: github.com/google/perfetto @ exact rev (raw fetch 200) |
| third_party/boringssl/src (`c7eb007f…`) | | only precompiler (gen_snapshot) and runtime/bin dart:io | yes: github.com/google/boringssl @ exact rev |
| third_party/zlib (chromium/src/third_party/zlib `3008c4b3…`) | | gen_snapshot, runtime/bin only | no standalone mirror; would need a BUILD.gn shim over system zlib or madler/zlib |
| third_party/double-conversion | in-tree | libdart | n/a |
| buildtools/linux-x64/clang (CIPD fuchsia clang) | | toolchain | substitute: symlink `buildtools/linux-x64/clang/bin` → `/usr/lib/llvm-18/bin` (has clang, clang++, llvm-ar, llvm-nm, llvm-readelf, llvm-objcopy, lld) — toolchain path hardcoded in build/toolchain/linux/BUILD.gn:43-51; flags used are clang-18-compatible (`-std=c++20`, only `-Wno-*` new-ish warnings) |
| buildtools/gn (CIPD `gn_version b8becbd4…`) | | | no apt `gn` in Ubuntu main/universe index for this host (apt can't locate); `generate-ninja` debs on archive.ubuntu.com are ≤2024-11 and **lack `path_exists()`** (used at runtime/bin/BUILD.gn:777, added to GN in 2025). Unofficial mirror **github.com/timniederhausen/gn** (master @ 2026-02-02, has `function_path_exists.cc`) is fetchable; build with `python3 build/gen.py --no-last-commit-position && ninja -C out` (not executed here — sandbox denied running fetched build scripts) |
| buildtools/ninja | | | system `/usr/bin/ninja` |
| tools/sdks/dart-sdk (CIPD) | | only for `compile_platform` (vm_platform.dill), kernel-service, etc. | **not needed**: use prebuilt SDK's `lib/_internal/vm_platform.dill` (same revision) |
| third_party/libcxx, libcxxabi (llvm.googlesource split repos) | | not used on Linux: `use_custom_libcxx=false` (build/config/c++/c++.gni), `use_flutter_cxx` only for msan/tsan/iOS/Android (BUILDCONFIG.gn:259-260) | n/a |
| sysroot (CIPD) | | only if `dart_sysroot="debian"`; plain `gn gen` leaves it "" (build/config/sysroot.gni) | n/a (cross builds would need Ubuntu `crossbuild-essential-arm64/riscv64` or a debootstrap sysroot + `target_sysroot`) |
| third_party/pkg/* (dart-lang packages on dart.googlesource) | | only for building Dart-side tools (CFE/platform dill/dartdev) | mostly on github.com/dart-lang/*, but unnecessary if platform dill + gen_snapshot come from the prebuilt SDK |

Build steps also need python3 + git (generate_version_cc_file runs tools/make_version.py, runtime/BUILD.gn:410-454).

### Verdict
**Feasible but not turnkey.** We cannot run `gclient sync` (googlesource + CIPD blocked). A minimal libdart build is
achievable with: system clang-18 (symlinked into `buildtools/linux-x64/clang`), system ninja, GN built from the
timniederhausen/gn GitHub mirror, `dart_support_perfetto=false` (or Perfetto from github), BoringSSL only if
building gen_snapshot, and a **hand-written ICU shim** (`third_party/icu/BUILD.gn` exposing `icuuc` over
unicode-org ICU sources or system libicu) — ICU is the one hard blocker with no mirror of the pinned Chromium fork.
Suggested args: `is_debug=false is_release=true is_clang=true dart_support_perfetto=false
dart_use_compressed_pointers=false target_cpu="x64"` and targets `runtime:libdart_jit runtime:libdart_aotruntime_product`.
Everything else an embedder needs for bring-up is already available without building: `vm_platform.dill`, host and
cross (arm64/riscv64) `gen_snapshot`s, all from the same revision (snapshot hash and SDK hash verified identical).
An alternative that sidesteps GN entirely is compiling the `.gni` source lists (vm_sources.gni, compiler_sources.gni,
etc.) with the Rust `cc` crate — the lists are plain data — but then the defines/configs from runtime/BUILD.gn
(`dart_config`, `dart_arch_config`, `dart_os_config`, product/aotruntime configs) must be replicated by hand.
Cross-arch libdart (arm64/riscv64) additionally needs a cross sysroot (not available from CIPD here).
