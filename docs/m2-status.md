# M2 status

Decisions and findings since the brief (docs/m2-dart-ui.md). Newer where the two disagree.

## Stage 1: binding layer (2026-10-28)

Done: `colored_box` reaches `Render` with no Dart error.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/colored_box out
    fl-dart run-app out/colored_box.so 800x600@2
    frame 1: view 0 800x600 layers=1 pictures=1 draws=1

* `fl-dart-ffi`: `Symbol` tables matched by name and argument count (a count mismatch stays unresolved rather than calling across a wrong ABI), `peer::attach`/`with`/`take` for native field 0, `guard` for `catch_unwind`
* Peers are tagged with their `TypeId`, read through a `repr(C)` header: a native given another class's object, or a disposed one, gets `None`
* A peer is freed once, by its finalizer. `dispose` natives take the value out first; the finalizer then frees an empty peer
* `fl-dart-vm`: `IsolateConfig::natives`, a per-isolate resolver for dart:ui `@Native`s; the FFI resolver is installed whenever `dart:ui` exists. `HandleScope::new_double`, `new_bool`, `new_f64_list`, `new_i64_list` (typed data, so `List<double>`/`List<int>` parameters accept them)
* `fl-ui`: the 21 natives of brief §3 plus `NativeSemanticsFlags::initSemanticsFlags`; 8 stubs, listed in `fl_ui::STUBS` with what each returns
* `fl-shell`: `_implicitViewId`, `_addView` (29 arguments, tight constraints), locales, semantics, accessibility, user settings, lifecycle, displays, in `RuntimeController`'s order
* `fl-dart run-app <libapp.so> [WxH[@dpr]]`: one line per frame; an app that renders none is an error
* `tools/flutter-app/build.sh`: libapp.so the way `flutter build linux --release` makes it (frontend_server `--aot --tfa`, release gen_snapshot), without the GTK shell
* ci.yml `Flutter apps (aot)`: Flutter 3.47.8 cloned at its tag, checked against commit `737ba8f` from Flutter's release index, Linux artifacts precached, cached between runs

Found on the way:

* The survey missed one native: `SemanticsFlags.none` is built while the root render object attaches, and its constructor is `@Native` `NativeSemanticsFlags::initSemanticsFlags` (25 arguments). Stubbed with an empty peer
* The scene is as the survey said: the root `TransformLayer`, one picture, one `drawRect`
* With `ScheduleFrame` a stub the app renders the warm-up frame and then has nothing left to do; the run loop returns and the process exits 0
