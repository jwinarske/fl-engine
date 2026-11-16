# M4 status

Decisions and findings since the brief (docs/m4-input.md). Newer where the two disagree.

## Stage 1: engine-to-framework messages (2026-11-16)

Done: the engine sends the framework a message and gets its response, or null when there is none to get.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/messages out
    fl-dart run-app out/messages.so 200x100 --message fl/echo=hello --message fl/never=x
    fl/echo: "hello"
    response fl/echo: "hello"
    fl/never: "x"
    frame 1: view 0 200x100 layers=0 pictures=0 draws=0
    response fl/never: null

* `fl_shell::Wake::send(channel, data, respond)` and `post`, from any thread: the engine's `DispatchPlatformMessage`. Queued, and dispatched by the loop one a step, after the isolate's own messages and before a frame: `_dispatchPlatformMessage(name, data, responseId)`, then the microtasks it queued. Data of `None` is a null `ByteData`
* Response ids are fl-shell's: 1 up, 0 for a message that wants no response, as the engine's. The continuation is kept by id from dispatch until the framework responds
* `PlatformConfigurationNativeApi::RespondToPlatformMessage`: the id and the response's bytes to the shell. Null is an empty response (the engine's `CompleteEmpty`), not the same as no bytes. Id 0, an id never given out and an id answered already are ignored, as the engine ignores them: a second response to one message does nothing
* The continuation runs on the UI thread inside the native, so it must not block. No persistent handle is held for a message
* Every message is answered once (brief §15): by the framework; with `None` as the loop returns, the dispatched first, then the queued; with `None` at once if sent after; and with `None` when a `Wake` no loop ran for is dropped
* The loop does not end while a message is queued. It does not wait for a response: a framework with nothing left to run cannot give one
* `run-app --message CHANNEL[=TEXT]`: sent in order once `main` has run, the text as UTF-8 or null without one; each response printed, `null`, its text quoted, or hex if it is not UTF-8. `app-cycle` posts them and drops the responses
* `apps/messages`, on dart:ui alone: answered with bytes, with none, with null, twice, by a listener set a frame later, by `ChannelBuffers` dropping a message for a newer one, and never. `messages_are_answered_every_way` checks its output line for line, both modes
* `every_native_is_called_by_an_app` runs it too: 15 apps

Found on the way:

* A channel with no listener keeps one message (`ChannelBuffers`' default capacity); a second answers the first with null. A build with asserts says so on stdout. The key and text input channels the framework listens to from its binding's start, so this matters only before `runApp`
* `_dispatchPlatformMessage` always responds, whether the sender wanted it or not; with id 0 the native returns at once

## Stage 2: pointer (2026-11-16)

Done: `flutter create`'s counter app, unmodified, counts three taps given as a script of input events, and its frame matches the stock engine's after the same three taps. Exit criterion 1.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/counter out
    fl-dart run-app out/counter.so 1200x900@3 --font ... --input apps/counter/test/taps.txt --png counter_tapped.png
    frame 74: view 0 1200x900 layers=3 pictures=3 draws=17
    input: frames=3 latency_us_max=17422 dropped=0

* `fl-input`, new, no dependencies: `PointerEvent`, `embedder.h`'s `FlutterPointerEvent` field for field, and `EventSink`, what a source delivers through from any thread: `pointer_events`, `now` on the frame clock, `wait_idle`, `is_closed`
* `fl_ui::pointer`: `PointerData`, its 36 fields in the engine's order, little-endian; an event made one as `FlutterEngineSendPointerEvent` makes it (a touch in contact while down or moving, whatever its buttons); and `Converter`, the engine's `PointerDataPacketConverter` ported with its 12 unit tests, case for case under their names
* Where the engine asserts, the event is dropped: a cancel, move, up, remove or pan-zoom update for a pointer never added. Two of the engine's own quirks are kept: a hover's buttons are remembered only if it moved, and a move's never
* `Converter` is not `Send`: its state is the UI thread's. The compile-fail case is stage 7's
* `fl_shell::Wake::input()` gives a source its `Input`, the `EventSink`. Events are queued, and the loop dispatches all that have arrived as one packet, after the isolate's messages and before a frame: converted, then `_dispatchPointerDataPacket`, then the microtasks it queued
* The loop does not end while an `Input` is alive, and ends an idle wait when it does end
* Bounded queue (brief §15): past `INPUT_LIMIT`, 4,096 events waiting, moves and hovers are dropped and counted; downs, ups, adds, removes and signals never. The converter moves a pointer to its next event, so the framework sees where it went, not the way
* The frame clock's zero is now the `Wake`'s creation, not the loop's start, so a source can stamp an event before the loop runs. Frame times are that much later; their spacing is the same
* Latency (brief §15): for each frame input asked for, from the oldest event it shows to the end of its `_drawFrame`. `run-app --input` prints the longest. 17.4 ms for a tap at 60 Hz on x86_64: the wait for the next tick, and the build. Held to two frames in the soak lane at stage 7
* `fl-input-script`, new, on fl-input alone: `tap`, `down`, `move`, `up`, `idle`, `wait`. Parsed whole before anything plays; an error names its line. 1 MiB, waits to 10 minutes, positions to 1e6. A touch is added as it goes down and removed as it lifts, as `fl_touch_manager` does
* `run-app --input SCRIPT`: played on its own thread from the app's first idle on. The app runs until the script has ended and nothing is left to do, so the last frame is the settled one
* `counter_counts_three_scripted_taps`: 74 frames, the last against `counter_tapped.png`, which `golden_test.dart` records from flutter_tester after `tester.tapAt` three times and `pumpAndSettle`. 1.065% differ, 0.183% by more than 32, the first frame's tolerance; the same in six runs
* ci.yml no-vm: fl-input's and fl-input-script's tests, and the boundary (D8): fl-ui and fl-shell depend on no backend, fl-input on nothing

Found on the way:

* The brief's "at a frame number or time" does not work for an app that draws only when asked: after its first frame the counter draws nothing until it is tapped, so no frame number arrives, and a time races the ripple. `idle` is what a test wants: the tap's frames are all drawn, then the next step
* The engine asks for no frame on a pointer event. `DefaultPointerDataDispatcher` hands the packet on at once, and the framework asks if what it did shows. The brief's "asks for a frame as the engine's does" (§4) is that: nothing
* Idle cannot see a Dart timer: a long-press or double-tap timeout is a message yet to arrive. A script that needs one waits with `wait`
* Moves are not merged per device within a frame (brief §15, "as the engine's dispatcher allows"): the default dispatcher does not, and resampling is the framework's. Only the limit drops any

Open: mouse, scroll, pan-zoom and more than one finger from the script are stage 5's; the converter and the packet already carry them.

## Stage 3: keys (2026-11-16)

Done: a script's keys, resolved against an XKB keymap and sent as the Linux embedder sends them, reach both of the framework's keyboards as flutter_tester's reach them. Exit criterion 3, keys.

    cargo build -p fl-dart-cli --features kms
    tools/flutter-app/build.sh $FLUTTER_ROOT apps/keys out
    fl-dart run-app out/keys.so 200x100 --input apps/keys/test/typing.txt
    raw down physical=0x70004 logical=0x61 character="a" repeat=false shift=false control=false alt=false meta=false
    data toolkit=GtkKeyHelper keyCode=0x61 scanCode=0x26 modifiers=0x0 unicodeScalarValues=0x61 specifiedLogicalKey=0x61
    key down physical=0x70004 logical=0x61 character="a" synthesized=false pressed=[0x61] locks=[]
    ...
    keys: events=40 handled=4 unhandled=36

* `fl_input::KeyData`, `embedder.h`'s `FlutterKeyEvent`, and `KeyEvent`, one native key event as the framework is to be told it (D2, D8): its `KeyData` in order, the synthesized first, and its message for `flutter/keyevent`. `EventSink::key_event` delivers one
* `fl_input::xkb`, the Linux embedder's key path below GDK, ported, with no keymap linked (D7). A source resolves a key into an `XkbKey`, GDK's `GdkEventKey` (keycode, keysym, its character, the state before the event, the group), and `Keys::handle` makes the `KeyEvent`:
  * `Tracker`: `fl_key_embedder_responder`. Which keys the framework has been told are down, repeats, an up with no down, and synthesized events where the state's modifiers and locks disagree with what was sent
  * `Layout`: `fl_keyboard_manager`'s `guarantee_layout`. Every letter and digit is some key's logical key on any layout: the key that types it, else the key that has it on a US layout
  * `RawChannel`: `fl_key_channel_responder` and `fl_key_event_channel`, the legacy message in GTK's shape, in one module (D2)
  * the tables of `fl_key_mapping.g.cc`, generated: 217 XKB keycodes to USB HID usages, 171 keysyms to logical keys, 49 layout goals. The XKB keycode is the evdev code plus 8, so the table serves a board's devices as it is
* Ported with their unit tests, case for case under their names: 16 of `fl_key_embedder_responder_test.cc`'s 17, converted by script (433 of its checks; the rest are of the mock's struct size); `fl_keyboard_manager_test.cc`'s layouts on its French and Russian data, modifiers and pressed state; `fl_key_channel_responder_test.cc`'s messages; `fl_key_mapping_test.cc`. The sources are the pin's: `.flutter/engine` holds the same files
* `fl_ui::key`: the `KeyDataPacket` as `_unpackKeyData` reads it; the two answers, one byte on `flutter/keydata` and `{"handled":true}` on `flutter/keyevent`; and `flutter/keyboard`'s `getKeyboardState`, a standard-codec map of physical to logical key
* The loop takes key events in their place among the pointer events, never dropped, and sends each as `fl_keyboard_manager` does: every `KeyData` on `flutter/keydata`, then the message on `flutter/keyevent`, each with its microtasks. Pointer events between two keys still go as one packet
* Handled if either answer says so, once both are in; an event with no `KeyData` of a real key, a release with no press, is handled as it stands. `Wake::keys` counts events, handled and unhandled; an answer that never comes is the loop's end. `fl_keyboard_manager_test.cc`'s eight cases of who answered are fl-shell's test
* `fl-platform-kms`, new: `XkbKeymap`, drmkit-input's `Keyboard` behind `fl_input::xkb::Keymap` (D3). `from_layout` names rules, model, layout and variant in full, so `XKB_DEFAULT_*` change nothing. Tested on the system's `us`, `fr` and `ru`
* The script: `key <name> down`, `up`, or neither for both; a key is named as Flutter names its place (`keyA`, `shiftLeft`, `numpad1`). A second down is a repeat. `run-app --keymap LAYOUT[:VARIANT]`, `us` by default; the keymap is made on the script's thread, which it does not leave (xkbcommon's state is not `Send`), and once before the app starts, so one that cannot be had is no run
* `apps/keys`: logs every `KeyEvent` and `RawKeyEvent` with the keys and locks the framework holds after each, handles key A, and asks `getKeyboardState` when F1 goes down. `keys_match_the_stock_framework` holds its lines against `test/typing_events.txt` and `test/french_events.txt`, which `flutter test` records for the same keys: 81 lines and 24, both modes
* ci.yml: the app lane installs `libxkbcommon-dev` and `xkb-data`, runs the apps with `--features kms` and fl-platform-kms's tests; no-vm lints fl-dart with the feature

Decided here:

* **The tracker runs in the source, not on the UI thread.** D8's boundary is the embedder API's shape, so what crosses it is `FlutterKeyEvent`s, and a platform with another keyboard model brings its own tracker with no change above `fl-input`. Brief §6 had the UI thread make key messages; it sends and awaits them
* **`getKeyboardState` is answered from what was dispatched**, not from the tracker: a down adds its key, an up removes it. The same map once the queue is empty, and never ahead of what the framework has seen
* **`kms` is a feature of fl-dart, off by default.** It links libxkbcommon, which the cross lanes' sysroots do not have. Without it a script with keys is refused before the app starts, saying so. On by default when the boards come through it (stage 6)
* **Rust floor 1.85 to 1.94.1**, as the brief has it (§7): drmkit's, and the pinned toolchain's
* **The logo key is `GDK_META_MASK`** (and Mod4, and `GDK_SUPER_MASK`) in the state a source reports. The embedder checks `GDK_META_MASK` for its Meta keys, which are the logo keys; the framework's GTK helper reads Meta from `GDK_SUPER_MASK`. What GDK itself sets for a held logo key on Wayland was not measured (D5: no recordings)

Differences from the embedder, each for its intent:

* The Caps Lock inference (`caps_lock_state_logic_inferrence`) is not ported, as D7 has it, nor its test, `TapLetterKeysBetweenCapsLockEventsReversed`. A source reads the state before the event from its keymap, which is the embedder's normal logic: xkbcommon locks on the first press and unlocks on the second release, and the tracker's stages follow with no synthesized event (fl-platform-kms's tests)
* A modifier down on two keys, the later let go, then gone from the state: the embedder releases the key it last went down on, already up, and the first stays down for good. Here the key still down comes up (brief §15, key state). A test of its own
* The embedder records the physical key of every logical key and reads back only the modifiers' and locks'. Only those are kept, so a source's keysyms cannot grow the tracker

Found on the way:

* flutter_tester sends the framework GLFW's message for Linux, not GTK's, and GLFW's logical key is the character typed: `A` for Shift and key A, `&` for digit 1 on a French layout. GTK's carries the layout's logical key (`specifiedLogicalKey`). So three `raw down` lines differ, in `logical` alone, and the test allows exactly that; the `KeyEvent` beside each and the `raw up` after agree
* drmkit-input's `Keyboard` does not say which layout of a keymap is in effect, and has no compose table. The group is 0, right for the one-layout keymaps `from_layout` makes; a keymap of several layouts, and dead keys for stage 4, need them from drmkit or from xkbcommon beside it
* A dead key (`us:intl`, the quote key) types no character and is logical key `"`, its US key: neither of its levels is Latin-1. Composing is text input's
* Enter and Escape carry a character, U+000D and U+001B: `gdk_keyval_to_unicode` gives one, and the embedder sends it

Open: text input (stage 4), which is where an unhandled key goes next. Keys held as a device goes or the focus does (brief §8, §15) need a device to go: stage 6. The script's keys are not in the soak yet (stage 7).

## Decisions taken 2026-11-16

D1 (devices and a script, no window), D4 (clipboard in the process), D5 and D6 (no system IME), each with what the stock Linux embedder does in its place (brief §13). D5 as taken: no recordings from the stock embedder; the engine's unit tests ported for each layer, and the same scripted input given to `flutter_tester` and to fl-dart end to end.
