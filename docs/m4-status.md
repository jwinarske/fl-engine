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

Open then, done at stage 5: mouse, scroll, pan-zoom and more than one finger from the script. Stage 5 also changed two things said above: the loop dispatches a packet for each delivery, not one for all that have arrived, and a script's events are timed by the script.

## Stage 3: keys (2026-11-16)

Done: a script's keys, resolved against an XKB keymap and sent as the Linux embedder sends them, reach both of the framework's keyboards as flutter_tester's reach them. Exit criterion 3, keys.

    cargo build -p fl-dart-cli --features kms
    tools/flutter-app/build.sh $FLUTTER_ROOT apps/keys out
    fl-dart run-app out/keys.so 200x100 --input apps/keys/test/typing.txt
    raw down physical=0x70004 logical=0x61 character="a" repeat=false shift=false control=false alt=false meta=false
    data toolkit=GtkKeyHelper keyCode=0x61 scanCode=0x26 modifiers=0x0 unicodeScalarValues=0x61 specifiedLogicalKey=0x61
    key down physical=0x70004 logical=0x61 character="a" synthesized=false pressed=[0x61] locks=[]
    ...
    keys: events=40 handled=4 unhandled=36 text=0

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

Open: keys held as a device goes or the focus does (brief §8, §15) need a device to go: stage 6. The script's keys are not in the soak yet (stage 7).

## Stage 4: text input (2026-11-16)

Done: a key the framework does not handle goes to text input, which types it into the text field that has the keyboard, composes a dead key's character, and performs the field's action on Return; the clipboard is answered. A `TextField` typed into, selected in with the mouse, copied from and pasted into goes through the editing values flutter_tester logs for the same edits and draws its frames. Exit criterion 2, and exit criterion 5 for `flutter/textinput` and the clipboard.

    cargo build -p fl-dart-cli --features kms
    tools/flutter-app/build.sh $FLUTTER_ROOT apps/text_field out
    cat apps/text_field/test/{typed,selected,pasted,submitted}.txt > edit.txt
    fl-dart run-app out/text_field.so 1200x900@3 --font Roboto=... --input edit.txt --keymap us:intl --compose en_US.UTF-8
    value text="caf" selection=3..3 composing=-
    value text="caf´" selection=4..4 composing=3..4
    value text="caf" selection=3..3 composing=-
    value text="café" selection=4..4 composing=-
    ...
    value text="café bar" selection=0..4 composing=-
    value text="café barcafé" selection=12..12 composing=-
    submitted "café barcaf"
    keys: events=32 handled=4 unhandled=28 text=9

* `fl_input::text`, the Linux embedder's text input below GTK, ported, with no OS code (D8):
  * `TextInputModel` and `TextRange`: the engine's (`shell/platform/common/text_input_model.cc`). The text in UTF-16 code units, its selection, and the range being composed
  * `TextInput`: `fl_text_input_handler.cc`. Which field has the keyboard and how it is configured; a key the framework left (`filter_key`): the input method first, then Home, End and Return; and an input method's signals (`ImEvent`: preedit start, changed and end, commit, delete surrounding). What the framework is to be told comes out as `Update`s: the editing state, one delta for a field that asked for deltas, or the field's action
  * `InputMethod`: the trait in GTK's `GtkIMContext`'s place. `Direct` composes nothing and types each key's character, by `GtkIMContextSimple`'s rule for a key outside a sequence: not a release, not with Control or Alt held, not a control character
  * `Clipboard`: the trait in `GtkClipboard`'s place, text only, and `MemoryClipboard`, the one in the process (D4)
* Ported with their unit tests: all 154 of `text_input_model_unittests.cc`, converted by script, under their names; and `fl_text_input_handler_test.cc`'s cases of what the handler tells the framework (the action, a multiline field's Return, Home and End with and without Shift, composing, surrounding text, the marked text's rect, the three delta cases, a changed configuration). Its 11 cases of GTK's input purpose and hints have nothing here to hold: they are an on-screen keyboard's
* `fl_ui::text_input`: `flutter/textinput` as `fl_text_input_channel.cc` has it, `JSONMethodCodec`. `setClient`, `updateConfig`, `show`, `hide`, `setEditingState`, `clearClient`, `setEditableSizeAndTransform` and `setMarkedTextRect` are decoded and answered; arguments that are not the method's are an error reply; a method the embedder lacks (`setStyle`, `setCaretRect`, autofill) is answered with null, which the framework's optional channel reads as no such method. The handler's three messages are encoded member for member as the embedder's
* `fl_ui`'s clipboard: `Clipboard.setData`, `getData` and `hasStrings` on `flutter/platform`, as `fl_platform_channel.cc` and `fl_platform_handler.cc`. A format other than `text/plain` is an error; `hasStrings` is whether the text is not empty; an empty clipboard's `getData` is null. `fl_ui::set_clipboard` takes a platform's own
* A JSON parser and writer of fl-ui's own for the two channels: bounded in depth (32), no panic on any bytes, what is not JSON is no call. No JSON crate comes in
* `KeyEvent::native`: the key as the keymap resolved it, kept for text input. The loop, once both of the framework's answers are in and neither handled the key, offers it to the handler (`fl_view`'s redispatch), on the UI thread, inside the framework's response. The handler's messages are dispatched ahead of the next key, so the framework has each key's text before the key after it, as the embedder's main loop has it. `Wake::keys` counts the keys text input took (`text`)
* `fl-platform-kms`: `Compose`, xkbcommon's compose tables as an `InputMethod` (D6). A sequence begun shows what was typed of it as the composing text, a dead key as its accent standing alone and the compose key as a middle dot, as GTK shows them; one finished takes that away, ends composing and commits the character, in GTK's order: three editing states. A sequence the table lacks is dropped with its last key. `--compose LOCALE` names the locale, the environment's by default
* The script: `mouse move`, `mouse down` and `mouse up` at a position, the primary button, as `fl_pointer_manager` sends a mouse: added by its first event, hovering until its button is down, never removed. Its device is 1, beside the touch's 0
* `apps/text_field`: a Material `TextField`, autofocused, that prints its editing value each time it changes. `text_field_edits_match_the_stock_framework` runs it four times, a part more of the edit each time, and holds the frames after the typing, the selection and the paste against flutter_tester's with Impeller (0.19%, 2.06% and 0.27% of pixels differ on lavapipe: glyph edges, and in the second the selection's highlight, one level off in one channel), and the 17 lines of the whole edit against `test/edit_events.txt`. A second script starts from `😄 ` (`--message fl/seed=...`), selects back over it with Shift and the arrows, cuts, pastes twice, deletes forward and types: 12 lines against `test/emoji_events.txt`, every offset counting the emoji's two units
* ci.yml: the app lane installs `libx11-data`, the compose tables, and runs `text_field`'s goldens and its test in both modes

Decided here:

* **The handler and the input method are on the UI thread**, in fl-ui beside the channel, not in the source. The embedder offers its input method a key only after the framework has left it and only while a field has the keyboard, so a shortcut's key or a key with no field never moves a compose sequence on. That order needs the framework's answer, which is the UI thread's. So `InputMethod` is `Send`, and `Compose` moves its xkbcommon state there (the one `unsafe impl` in fl-platform-kms: the state, its table and their context are held by the one value, and used through `&mut`)
* **The reference types as the embedder does.** flutter_tester has no engine text input: `TestTextInput.updateEditingValue` sends the framework the editing state on the same channel. The test sends the states the Linux embedder sends for each key, the three of a composed character among them, and sends as keys the ones the framework handles itself (the shortcuts, the arrows, Backspace, Delete). What is held is that fl-engine's keys, through keymap, tracker, framework, handler and compose table, come to the same values
* **`--compose` in the test names `en_US.UTF-8`.** The table is the system's file for a locale, and the test's is not left to the runner's environment
* **No `text` or `compose` step in the script**, which brief §4 listed. Text is typed by keys, through the keymap and the compose table, which is the path a board's keyboard takes; a step that put text in directly would go around it

Differences from the embedder, each for its intent:

* The model refuses a selection or a composing range that is not inside the text, as the original does, and where the original then keeps ranges that may be past the new text's end, or sums a caret offset unchecked, the ranges go to the text's start or are held inside it. Nothing indexes with what the framework sent (brief §15). The brief said clamped; refused is what the embedder does, and the brief now says so
* An offset missing from `setEditingState` is 0, and a missing text an error reply; the embedder reads both through null
* `preedit-changed`'s caret is in UTF-16 code units; GTK's is in characters, which the embedder adds to a UTF-16 offset as they are
* GTK's Ctrl+Shift+U hex input is not ported: it is `GtkIMContextSimple`'s own, not the compose tables'
* A sequence the table lacks is dropped, with no beep

Found on the way:

* The selection's highlight, a translucent fill, blends to 193 in one channel on lavapipe where Impeller's golden and fl-engine on a hardware GPU both have 194: 0.23% of the frame differs on one driver and 2.06% on the other. The tolerance is lavapipe's, CI's; a frame's tolerance is to be measured there (`VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`)
* The embedder's delta for Home or End under the delta model is the whole text inserted at the old caret (`TextEditingDelta` test: old `Flutter`, delta `Flutter`, 7 to 7), which is no description of a caret moving. Its own test holds it to that, so it is ported as it is. The framework handles Home and End itself in a focused field, so the handler's path is reached only when it does not
* The framework cannot set a composing range through `setEditingState`: the handler's `SetComposingRange` is refused unless the model is composing, and nothing but the input method starts that. Ported as it is
* The framework says a key is handled only as it goes down: the copy, End, the paste and Backspace count 4 handled of their 8 events, and each release is offered to text input, which has nothing to do with one
* Return in a one-line field is not handled by the framework: it reaches the handler, which sends `TextInputClient.performAction`, and the field submits. Space is not handled either, and types
* A message sent with `run-app --message` arrives after the first frame, by which time an autofocused field has the keyboard: the seed's text goes back to the engine as `setEditingState`, which is the path the second script is there for

Open: `flutter/mousecursor` (exit criterion 5's last channel) comes with the cursor, stage 6; other `flutter/platform` methods are still dropped unanswered, as before. A system input method (IBus, Fcitx) is a later `InputMethod` (D6). Typing in the soak, its build-time budget, and fuzzing the JSON and the editing-state decoder are stage 7's lanes; the decoders' malformed-input tests are here.

## Stage 5: scroll and touch (2026-11-16)

Done: a mouse with its buttons and its wheel, more than one finger, and a touchpad's pan, pinch and rotation, from the script, sent as the Linux embedder's managers send them. `apps/pointer` sees the pointer events flutter_tester is sent for the same, and `apps/scroll` comes to rest where flutter_tester's list does. Exit criteria 3 (pointer) and 4.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/scroll out
    fl-dart run-app out/scroll.so 1200x900@3 --input apps/scroll/test/scroll.txt
    settled 159.0      the wheel, three notches
    settled 259.0      a finger's drag
    settled 711.9      a fling
    settled 817.9      a touchpad's pan
    settled 764.9      the wheel, a notch back

* `fl_input::pointer`, the embedder's three managers below GDK, ported, with no OS code (D1, D8). A source says what a device did, in physical pixels, and gets the `PointerEvent`s to deliver:
  * `Mouse`: `fl_pointer_manager`. Added by its first event, a down with its first button and an up with its last, moves between and hovers with none; a stylus's buttons are its own three; it is removed as it leaves the view unless a button is down
  * `Scrolling`: `fl_scrolling_manager`. A wheel's notch is a scroll signal of 53 logical pixels; a touchpad's two-finger scroll is a pan-zoom gesture whose pan adds up, the other way; a pinch and a rotation are one gesture between them, where the mouse last was
  * `Touches`: `fl_touch_manager`. Each finger a pointer of its own, with the lowest id none has, added before its first event and removed with its last
* Ported with their unit tests under their names: `fl_pointer_manager_test.cc`'s 23 (its five of one button each are one case over the five), `fl_scrolling_manager_test.cc`'s 7, `fl_touch_manager_test.cc`'s 1. The scroll tests' GDK directions are a source's to turn into deltas
* The devices are the embedder's: the mouse is device 0, a touchpad's gestures device 1, a touch `2 << 28` and its id. The script's touch was 0 and its mouse 1 until here
* The script: a finger's number after a touch's position; `mouse down` and `up` with a button's name, and `mouse leave`; `scroll X Y DX DY` in notches; `pan X Y DX DY` and `pan end X Y`; `zoom` and `rotate` with `begin`, a value, or `end`. `Script::play` takes the view's scale, which a notch is measured in
* `apps/pointer`: a `Listener` over the view logs every event, and the router's adds and removes. `pointers_match_the_stock_framework`: 33 lines against `test/pointers_events.txt`, which `flutter test` records from the events the converter makes of the embedder's, sent with `TestPointer`
* `apps/scroll`: a list of 100 rows logs its offset as it moves and where each scroll ends. `scroll_settles_where_the_stock_framework_does`: the five offsets against `test/scroll_settled.txt`, each within a logical pixel; they are the same to the tenth printed
* ci.yml: `pointer` and `scroll` in the goldens step; their tests need no keymap and run in the default build too

Decided here:

* **A delivery is a packet.** `EventSink::pointer_events` is `FlutterEngineSendPointerEvent`: its events go to the framework as one packet, and the loop keeps the packets apart, each with its microtasks. Stage 2 dispatched all that had arrived as one. The Linux embedder sends every event in a packet of its own, and the script now does. It shows: the framework resolves a one-recognizer arena in a microtask, so a pan's start and its first update in one packet lose the update to the drag's start (26.5 of 106 pixels, measured), and in two they do not
* **A script's events are timed by the script.** An event's time is when the script began plus its `wait`s, not the clock when the step ran, so a fling's speed is what the script says: five moves 8 ms apart are 8 ms apart, whatever the sleeps came to. `idle` brings the script's time up to the clock's. Without it the fling could not be held to a pixel
* **No `enter` step.** The manager has it; a mouse that moves or presses is added the same

Differences from the embedder, each for its intent:

* A zoom's and a rotation's events are stamped by the source, on the frame clock like every other; the embedder stamps them with the wall clock (`g_get_real_time`), which is not the clock its other events are on
* Past 128 fingers at once the embedder's ids wrap to 0 and its table grows; here the rest share id 0 and are not remembered (brief §15)
* A touch that is cancelled is not handled by the embedder (`GDK_TOUCH_CANCEL` falls through), so its pointer stays down; `TouchAction` has no cancel yet. A device removed mid-gesture is stage 6's

Found on the way:

* The framework takes a pointer to have stopped by its own clock, not by the events' times: a finger that rests 200 ms before it lifts is not flung because 200 ms went by on `samplingClock`. Under flutter_tester that clock moves only when the test pumps a duration, so the reference pumps the script's waits
* A hover to where a pointer already is does not reach a `Listener`: the mouse's first hover, where it was added, is no line
* A touchpad's pinch begins where the mouse last was, and the converter hovers the touchpad's pointer there from where its pan was: a `PointerHoverEvent` of kind trackpad
* The wheel scrolls at once, with no animation, and each of its events ends a scroll

Open: the boards' devices behind the same managers, with the cursor and `flutter/mousecursor`, are stage 6. A wheel's high-resolution steps and a touchpad's kinetic scrolling are libinput's to report there.

## Stage 7: lanes (2026-11-16)

Taken before stage 6: no board was to hand (the Pi 5 off the network, the RK3566 with no panel on its HDMI and a Vulkan driver that does not draw, the VisionFive 2 with no login), and the lanes need none. Exit criteria 6 and 7, and 8 as far as the scripted source goes; the lanes take the boards' devices when stage 6 brings them.

* **Soak with input** (`input_cycles_stay_flat`, soak.yml's AOT leg, now built with `--features kms`): apps cycled in one VM with a script of input each lifecycle, its thread started and joined each time, held to the app soak's bounds: RSS over the last nine tenths within 512 kB, descriptors the same, no persistent handle left by a key's two answers or a message's, no motion dropped
  * counter, three taps a cycle at 60 Hz, 20 cycles: the 60 frames input asked for show it in 16,989 us at the 90th percentile, 17,056 at most, against the brief's two frames (33,334)
  * pointer, 300 cycles of every device kind; scroll, 20 cycles of wheel, drag, fling and pan
  * keys, 300 cycles: 12,000 key events through the tracker and both of the framework's keyboards
  * text_field, 100 cycles: 10,000 characters typed, a frame each, frames built in 891 us on average against M3's 4,000
* `app-cycle` reports what `--input` did over all its cycles: the frames input asked for and the latency's median, 90th percentile and longest, the motion dropped, and the keys by how they were answered
* **Fuzz**, three targets in the decoders' style (a seeded generator, pure Rust, no libFuzzer), short in ci.yml and long under ASan in sanitize.yml:
  * fl-input-script: the apps' scripts and one of every step, mutated. What parses is played, its waits taken out, into a sink that holds every position, distance and device to the script's bounds; what does not is an error naming a line of the text
  * fl-input: the text input handler given editing states, keys and an input method's signals in any order, its ranges inside its text after each and in what it tells the framework; and the key path and the three pointer managers given keys no keymap gives and positions no device reports, more fingers than ids among them
  * fl-ui, in the crate, where the decoders are: the framework's calls on `flutter/textinput` and the clipboard's on `flutter/platform`, mutated, and calls made up of JSON of any shape under the methods' names. Each is answered with a reply that is itself an envelope, or is no call; a key is typed after each on whatever it left
* **Sanitizers** (sanitize.yml, ASan and TSan, now with `--features jit,kms`): counter tapped, pointer, scroll, keys and text_field's whole edit under `app-cycle`, the script's thread delivering to the loop's queue and waiting on its idle
* **Compile-fail** (exit criterion 6): `fl_ui::pointer::Converter` cannot be given to another thread (fl-ui's first trybuild case, in the no-vm lane); and fl-input holds its events, an `InputMethod`, a `Clipboard` and an `EventSink` to `Send`, in a test that is its bounds
* **Every limit crossed** (exit criterion 7, brief §15): the queue's 4,096 (fl-shell), the script's megabyte, waits, positions, notches and fingers (fl-input-script), JSON's depth of 32 (fl-ui), the 128 touch ids (fl-input); offsets outside a text refused (fl-input, fl-ui)

Found on the way:

* **The fuzzer found one.** An input method that composes text with no start signal, then replaces it with less, left the model's composing range past the end of the text, and the handler would have told the framework that range as it was. The original has the same, reachable only by signals out of order, which GTK's do not come in. The model now holds both ranges inside the text after every change; the 154 ported cases are unchanged by it
* A text field whose text is wider than it is scrolls to its caret after every key, an animation of 100 ms: 100 frames at 1 kHz. Typing 100 characters into the soak's field drew 7,900 frames a cycle and took 10 s (941 s for the run), and `app-cycle`'s own list of build times, eight bytes a frame, was the 6 MB the run grew by. The soak clears the field every ten characters, by Ctrl+A and Backspace, which the framework handles
* Drawn, the tapped counter's glyph atlas is still filling at 20 cycles, as its own soak's is over its first thirty; the input run is the UI side only

Differences from the brief:

* **Exit criterion 6 reads "the converter and editing state are not `Send`".** The converter is not. The editing state is: the handler sits behind fl-ui's mutex with the channel that changes it (stage 4), so it is the UI thread's by where it is called from, not by its type. Its input method must be `Send` to be put there
* **The latency bound is on the 90th percentile**, not on every frame: one late wake of a shared runner's thread would fail the lane for nothing the engine did. The longest is reported

Open: stage 6, the boards. With it the lanes get the devices' thread under TSan, hotplug, and tap-to-frame latency on the Pi 5.

## Stage 6: the display (2026-11-16)

The first part of stage 6, with the Pi 5 back: `fl-dart run-app --kms` draws an app to a display held through the seat, its frames begun at the display's vertical blanks. Devices, the cursor and `kms` by default are the parts to come.

* **`fl-platform-kms::Seat`**: the session, through drmkit-session (libseat). libseat's connection is one thread's, so the seat has a thread, which opens what it is asked for and hears the session paused and resumed. A descriptor it hands out is a duplicate of the seat's own. No seat is an error that says what is missing; nothing opens a node directly
* **`fl-platform-kms::Display`**: emblema's `ScanoutOutput` over drmkit (D9). drmkit-display finds the connector's CRTC and primary plane (`--connector HDMI-A-1`, or the first with a display), drmkit-core holds the properties and makes the commits, drmkit-modeset delivers the flips. emblema's ring of buffers is imported as framebuffers; each frame is one nonblocking atomic commit, the first setting the mode
* **fl-raster, feature `scanout`**: the raster thread's device with emblema's `DrmScanoutTarget` for a target, in the offscreen surface's place. What both share (the context, the images, the glyph atlas, the recording of a scene) is one `Device`. The thread waits on the display's flips, not on a clock
* **Vsync is the display's** (`Wake::vsync`): each flip gives the loop the time of a vertical blank, from the kernel, and the mode's period, and frames are begun on the blanks from then on. A blank that moves by its jitter from one flip to the next is not begun twice: the next tick is at least half a period after the last
* **A switch of terminals** takes the card: frames drawn meanwhile are dropped and counted, at the clock's rate; on the switch back the card is taken again on the seat's new descriptor, the output says everything is to be rebuilt, and the ring is made again. A commit refused for want of DRM master ahead of the seat's saying so is the same; one refused before any flip is an error naming a compositor
* `run-app --kms [--connector NAME] [@dpr]`: the view is the mode's size and rate. `kms: card=… mode=WxH@Hz flips=… vblanks=… dropped=…` at the end

Measured on the Pi 5 (Raspberry Pi 5 Model B Rev 1.0, Debian 13, Mesa 26.2.2 V3D 7.1.7.0, seatd 0.9.1, over ssh), cross-built on x64:

| display | mode | run | flips | vertical blanks spanned |
|---|---|---|---|---|
| HDMI-A-1 (`vc4`) | 1280x1440 at 59.995 Hz | `ticker`, 600 frames, three runs | 599, 599, 599 | 598, 598, 598 |
| DSI-2 (`rp1-dsi`) | 800x1280 at 60.079 Hz | `ticker`, 600 frames, three runs | 599, 599, 599 | 598, 598, 598 |

* A frame at every blank for ten seconds on each controller, none missed: what emblema measured with its own drm-rs output (its `docs/on-a-board.md`) holds through drmkit, as D9 was to check. The 600th frame's flip is after the report
* `counter` at DPR 2 on HDMI, tapped three times by script: 73 frames over 202 blanks, drawn only when asked for; the frames the taps asked for done within 15,272 us of their events on the UI thread
* A switch to terminal 2 and back, a second fl-dart on that terminal switching back: 204 frames dropped while away, the card retaken, 395 of 600 frames shown, no error. Driven by a hook since removed: nothing at the board can ask for the switch until a keyboard's keys come through the seat

Found on the way:

* **emblema's submission that returns a fence does not take a multisampled pass**, and a canvas antialiases by multisampling. So each frame is drawn and waited for on the raster thread and committed with no fence, where the kernel would otherwise latch the flip when the drawing is done. It keeps every blank at these sizes; the fence is emblema's to add
* **emblema renders to `AR24`, `AB24` and `XR30`.** A primary plane that takes only `XR24` does not negotiate. The Pi 5's two take `AR24`
* A disabled session cannot ask seatd to switch back (`EAGAIN`); the terminal it left has to
* A Pi's `v3d` is a card with no connectors, and refuses universal planes: skipped, not an error
* Over ssh the seat is had: seatd binds a client to the terminal in front. Its socket is the `video` group's on Debian, so that group is still needed there, for the socket and not for the card

Differences from the brief:

* **pkg-config at build time, for the `kms` feature only**: libseat-sys finds libseat with it. The workspace's rule against it stands for everything the default build has; brief §7 has the platform's libraries "as they are", linked and not built. A cross build gives it `.pc` files for the board's libraries
* fl-platform-kms depends on emblema's `emblema-present-drm` and `emblema-hal`, for the trait, and on no rendering backend
* **No frame is read back from a display**: `--png` and `--frames-dir` with `--kms` are refused

Tests: the clock on a display's blanks (fl-shell); a mode's rate from its timings, the flips counted, a kernel timestamp as an `Instant` (fl-platform-kms); and the raster thread over an output that is no card, which takes the ring's buffers, loses them once as a retaken card does, and has every frame committed and every buffer given back (fl-raster's `scanout` test). That last needs a driver that exports dma-bufs with modifiers: lavapipe at Mesa 26.2 does, and the hosted runner's 25.2 does not, so in the app lane it says it is skipped and checks nothing yet; it passes here on lavapipe 26 and on a hardware driver. KMS itself is the boards'.

Open, for the rest of stage 6: the devices through drmkit-input under the same seat, with the keys that switch terminals; the cursor plane and `flutter/mousecursor`; hotplug; `kms` by default and the board kit's display check; the VisionFive 2. Then drmkit-input's gaps: touchpad gestures, and devices added and removed.

## Stage 6: the devices (2026-11-16)

The second part of stage 6: with `--kms` and no script, the seat's keyboards, mice, touchpads and touch screens are the app's input. Written and unit-tested; **not yet seen with a device's events on a board** (see the end).

* **`Seat::read_devices`**: libinput's devices, through drmkit-input, opened through the session by a `LibinputInterface` over it, on the seat's thread. They are a loop's way in, so the loop runs until it is stopped; `release_devices` lets it go
* What libinput says goes the script's path: the key tracker, and fl-input's pointer, scrolling and touch managers, each event a packet of its own
  * **keys**: the evdev code through the XKB keymap (`--keymap`). A held key repeats by drmkit-input's `KeyRepeater` (600 ms, then 25 Hz), each repeat a down for a key that is down, which is what a repeat is to the embedder
  * **Ctrl+Alt and a function key** is the seat's: the keymap gives `XF86Switch_VT_n`, the seat is asked for that terminal, and the framework is not told of the key
  * **a mouse** moves a pointer that starts in the middle of the view and is held to it; a tablet's or a touch screen's extent is mapped onto the view
  * **a wheel**: 15 degrees a notch. **A touchpad's two fingers**: a pan, ended by libinput's scroll of nothing
  * **a finger**: its slot, from its down to its up, which is where it last was
* **A switch of terminals**: what is down comes up (keys, fingers, a pan, the mouse leaves), the devices are closed, and opened again on the switch back. The same when the devices are let go
* Bounded on the way in (brief §15): positions to the view, non-finite ones to zero; a key code past the keymap's range dropped; 64 fingers remembered
* `--kms` with `--input`: the script is the input and the devices are not read, so the run ends with the script

Seen on the Pi 5: the six event nodes opened through the seat by the process, `ticker` still at a frame a blank with them read (239 flips, 238 blanks). Tests, with no device: the state driven with drmkit-input's events (fl-platform-kms, six cases: the pointer held to the view, buttons, a wheel and a pan, a finger, the terminal keys, everything let go).

Differences from the brief:

* **A device's time is the loop's clock when it is read**, not libinput's stamp, which is of another epoch
* **A cancelled touch ends as an up.** `fl_touch_manager` has no cancel; brief §15's "ends its pointers with `cancel`" for a device removed needs one added to fl-input, with drmkit-input's device events

Open: nothing here has moved a real device. `/dev/uinput` is root's on the Pi and `sudo` wants a password, so no synthetic device can be made over ssh; it needs a hand on the mouse, or a udev rule giving the `input` group `/dev/uinput`, with which keys, the terminal keys, fingers and unplugging can all be driven from here. The pointer is not drawn until the cursor plane.

## Decisions taken 2026-11-16

D1 (devices and a script, no window), D4 (clipboard in the process), D5 and D6 (no system IME), each with what the stock Linux embedder does in its place (brief §13). D5 as taken: no recordings from the stock embedder; the engine's unit tests ported for each layer, and the same scripted input given to `flutter_tester` and to fl-dart end to end.
