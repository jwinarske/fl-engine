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

Open: `flutter/mousecursor` (exit criterion 5's last channel) came with the cursor, stage 6; other `flutter/platform` methods are still dropped unanswered, as before. A system input method (IBus, Fcitx) is a later `InputMethod` (D6). Typing in the soak, its build-time budget, and fuzzing the JSON and the editing-state decoder are stage 7's lanes; the decoders' malformed-input tests are here.

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
* A touch that is canceled is not handled by the embedder (`GDK_TOUCH_CANCEL` falls through), so its pointer stays down; `TouchAction::Cancel` is stage 6's, for a device removed mid-gesture

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

The second part of stage 6: with `--kms` and no script, the seat's keyboards, mice, touchpads and touch screens are the app's input. Seen on the Pi 5 with devices made through `/dev/uinput` (see the end).

* **`Seat::read_devices`**: libinput's devices, through drmkit-input, opened through the session by a `LibinputInterface` over it, on the seat's thread. They are a loop's way in, so the loop runs until it is stopped; `release_devices` lets it go
* What libinput says goes the script's path: the key tracker, and fl-input's pointer, scrolling and touch managers, each event a packet of its own
  * **keys**: the evdev code through the XKB keymap (`--keymap`). A held key repeats by drmkit-input's `KeyRepeater` (600 ms, then 25 Hz), each repeat a down for a key that is down, which is what a repeat is to the embedder
  * **Ctrl+Alt and a function key** is the seat's: the keymap gives `XF86Switch_VT_n`, the seat is asked for that terminal, and the framework is not told of the key, nor of its up, by when the modifiers may be up themselves
  * **a mouse** moves a pointer that starts in the middle of the view and is held to it; a tablet's or a touch screen's extent is mapped onto the view
  * **a wheel**: 15 degrees a notch. **A touchpad's two fingers**: a pan, ended by libinput's scroll of nothing
  * **a finger**: its slot, from its down to its up, which is where it last was. One whose device goes, or whose session does, is canceled (`TouchAction::Cancel`, added to fl-input's touch manager: a cancel and a remove), so a tap under way is given up and not made
* **A switch of terminals**: what is down comes up (keys; fingers are canceled, a pan ends, the mouse leaves), the devices are closed, and opened again on the switch back. The same when the devices are let go
* Bounded on the way in (brief §15): positions to the view, non-finite ones to zero; a key code past the keymap's range dropped; 64 fingers remembered
* `--kms` with `--input`: the script is the input and the devices are not read, so the run ends with the script

Seen on the Pi 5, on HDMI, with a keyboard, a mouse and a touch screen made through `/dev/uinput` by a small tool run over ssh (the `input` group given the node by a udev rule), each plugged in after the app started:

* **`pointer`**: the mouse added where it moved to, a drag with the first button, the second button, a wheel's notch as a scroll of 53; two fingers as two pointers, each added, down, moved, up and removed; the touch screen unplugged with a finger down: canceled and removed
* **`keys`**: `a`, Shift and `A`, `s` held (a down and three repeats in 700 ms), each as the raw event, the embedder's data and the framework's key event
* **Ctrl+Alt+F2 and back with Ctrl+Alt+F1**: Control and Alt down, the session paused, both up and no F2 told; the session resumed, the keyboard read again, `b` typed
* `ticker` with the devices read: 239 flips over 238 blanks, none dropped

Tests, with no device: the state driven with drmkit-input's events (fl-platform-kms, six cases: the pointer held to the view, buttons, a wheel and a pan, a finger and its cancel, the terminal keys, everything let go).

Differences from the brief:

* **A device's time is the loop's clock when it is read**, not libinput's stamp, which is of another epoch
* **A touch has a cancel `fl_touch_manager` has not**, for brief §15's device removed

Open: a touchpad's pan and a tablet were not made on the board (a touchpad needs more of a device than the tool describes), and no real keyboard or touch screen has been tried, the mouse on the board aside.

## Stage 6: the cursor (2026-11-16)

The third part of stage 6: a mouse's pointer is drawn, on the card's cursor plane, in the shape the framework asks for.

* **`flutter/mousecursor`** (fl-ui `mouse_cursor`, exit criterion 5's last channel): `activateSystemCursor` decoded from `StandardMethodCodec`, its kind turned into a cursor theme's name by the Linux embedder's table (fl-input `cursor_name`, 35 kinds, the default for one it has not), and answered: success, or the embedder's `Bad Arguments` with no kind. Another method is no method. The platform's cursor is behind `fl_input::MouseCursor`; with none set the calls are answered and nothing is shown
* **`Cursor`** (fl-platform-kms): drmkit-cursor's renderer on the CRTC of the display, on a thread of its own with the seat's descriptor for the card duplicated. XCursor themes are found and read by drmkit-cursor, in Rust (`XCURSOR_THEME`, `XCURSOR_PATH`); the size is 24 logical pixels at the view's scale, within the card's cursor size
* **It follows the mouse**: the device source sends each position it holds the pointer to. Nothing is shown until the mouse first moves; a finger down hides it until the mouse moves again, and so do the devices going. `none` hides it; an animated cursor is stepped while it is shown
* **A switch of terminals**: the renderer is paused with the session and resumed on the card's new descriptor, with what was asked meanwhile
* Asks pile up in a channel and are taken together, the last position the one shown; a shape's name is bounded to 64 bytes before it is sent, and drmkit refuses one that is not a file's name

Seen on the Pi 5, on HDMI, with the mouse made through `/dev/uinput`:

* the card's cursor plane (type Cursor) on the display's CRTC with a framebuffer once the mouse moved, and again after Ctrl+Alt+F2 and back
* `ticker` over 420 frames with the mouse moved 400 times: 419 flips over 420 blanks, none dropped; without the mouse 419 over 418. Showing the cursor the first time costs two blanks, moving it none
* `counter`: the pointer taken to the button and clicked

Tests: the channel (a kind shown by its name, every call answered, a call cut short or changed anywhere), the kind table, the asks taken together and held over a pause, and the device source's moves and hides (fl-ui, fl-input, fl-platform-kms).

Differences from the brief:

* **The cursor is not committed with the frame** (D9). It is moved by the legacy cursor calls, which the kernel applies without waiting for a blank; drmkit-cursor's atomic path commits the plane alone and blocking, and on vc4 that cost the scanout blanks while the pointer moved. A card whose cursor those calls do not drive has no cursor: the run says so and goes on
* **No cursor with `--input`**: a script's pointer is not drawn, the devices not being read

Open: the cursor's shape has not been looked at on the screen by a person (the plane and its framebuffer were read from the card).

## Stage 6: hotplug and the board kit (2026-11-16)

* **Hotplug** (brief §15), on the Pi 5 with devices made through `/dev/uinput`: a keyboard unplugged with a key down, the key's up reaches the framework; a mouse unplugged with a button down, the pointer's up; a touch screen unplugged with a finger down, a cancel (above). libinput lets go of what a removed device held, and the device source passes it on. A device plugged in while the app runs is read: every device in these checks was
* **The board kit's display check** (M2's exit criterion 4, brief M2 §11): the kit has `fl-dart-kms`, fl-dart with `--features kms`, and `ticker` for the target; `run.sh` shows 300 frames and passes with none dropped and no blank missed. A board with no display or no seat, or without the libraries, is a SKIP. On the Pi 5: 299 flips, 0 blanks missed, the nine other checks as before
* **The board kit's input check** (brief §11), `run.sh --input`: `keys` and `pointer` each shown for 20 seconds while someone presses a key and clicks or touches; a key down and a pointer down must reach the framework. On the Pi 5 with the synthetic devices: both pass
* `tools/flutter-app/build.sh` compiles an app for riscv64 too, with the Dart SDK's cross gen_snapshot
* cross.yml installs the target's libseat, libinput, libudev and libxkbcommon from Ubuntu's ports archive for the kit's build, and the pinned Flutter release for its apps. Under qemu the display's check is skipped

Differences from the brief:

* **`kms` stays a feature that is asked for**, not the default: the default fl-dart links none of the four libraries, so the kit's other checks run on a board without them, and a host builds without their headers. The kit carries both binaries

Open: the VisionFive 2, on which nothing of stage 6 has run. A display plugged in or unplugged while an app runs is not handled: the app keeps the connector it started on.

## Stage 6: a touchpad's pinch, and a mouse unplugged (2026-11-16)

drmkit-input's two gaps, closed there (drmkit 0.1.4: `InputEvent::Gesture` and `InputEvent::Device`) and read here.

* **A pinch** is the embedder's zoom and rotation at once, as GTK's two gestures are: a pan-zoom that starts with the pinch, an update with the fingers' distance against where they began and one with the turn, and an end. libinput says each step of the turn in degrees; the embedder is told the whole, in radians, clockwise as the framework's is
* A pinch that is canceled ends: the framework has no cancel for a pan-zoom. One the devices going cuts short ends too
* A pinch is said where the mouse last was, and before any mouse has moved that is where the pointer starts, the middle of the view. The embedder's starts at the corner
* A swipe of three fingers or more and a hold are nothing to the framework, as they are nothing to the embedder
* **A mouse unplugged** takes its pointer from the framework and its cursor from the screen. libinput let go of a removed device's keys, buttons and fingers already; the pointer it had moved stayed, hovering where it was left, with its cursor. The device is known by its name, which each move carries: two mice of one name are one to this, and unplugging either takes the pointer until the other moves

Seen on the Pi 5 with a touchpad and a mouse made through `/dev/uinput`, in `pointer`: a pinch out as a trackpad pointer added, a pan-zoom started, nine updates to a scale of 2.3 and its end; the mouse added by its first move, and removed as it was unplugged.

Tests (fl-platform-kms, two more cases): a pinch's zoom and turn as one gesture, with an update that has no begin, a device that says nothing sensible, a cancel and the devices going; the pointer leaving with the mouse that moved it, and not with a keyboard or another mouse.

Not seen: a real touchpad, and a rotation from a device (the script's fingers moved apart on a line).

## Stage 6: latency on the board (2026-11-16)

Stage 7 left tap-to-frame latency on the Pi 5 for stage 6 (brief §15: from the oldest event a frame shows to the end of its `_drawFrame`, held to two frames).

* `run-app --kms` prints the `input:` line a script's run prints, for the seat's devices: the frames input asked for, the longest latency, the motion dropped
* On the Pi 5 at 1280x1440 and 60 Hz, `counter` with a mouse made through `/dev/uinput` taken to the button and clicked, three runs of 100 frames: 20 frames asked for by input in each, the longest 16.7, 16.7 and 16.2 ms, none dropped. Under one frame of 16.7 ms: an event waits for the next blank, and the build is short

Not measured: from the device to the screen. The blank the frame is shown at is one more after the end of `_drawFrame`, and libinput's own delay is before the event.

## Stage 6: the cursor looked at, and the screen read back (2026-11-17)

The cursor's shape had not been looked at by a person (above). Now it has, and what the display shows can be kept.

* **`apps/cursors`**: a cell for each of the framework's 35 system cursors, saying its kind and the cursor theme's name for it; the cell the mouse is in is printed
* **Looked at on the Pi 5**, on HDMI at 1280x1440 with a real mouse (Logitech M705) and the machine's theme (Adwaita, which has a file for all 34 that show): the cursor changes in each cell
* **`every_system_cursor_is_asked_for_by_its_theme_name`** (app lane): a script's mouse into each cell in turn. The framework asks for all 35, each by its kind, and the engine's name for each is the cell's. `run-app --input` prints what is asked, `mouse cursor: NAME`, a script's mouse having no cursor to show
* **`run-app --kms --capture out.png`**: Print Screen writes what the display shows to a PNG, the first as named and those after numbered, and is no key of the framework's while that is asked for. `fl_platform_kms::capture` reads every plane on the display's CRTC back from the card and composes them, the cursor's among them, through drmkit-capture, on a thread of its own with the seat's descriptor

Found on the way:

* **Red and blue were exchanged on the Pi 5's screen**, in every frame since stage 6's first. The first capture read back showed it: a cell drawn `2D3748` was scanned out as `48372D`. emblema named its ten-bit buffer `XR30` to KMS, and its bits are `XB30`'s: Vulkan's `A2B10G10R10`, red lowest. vc4 offers both and took what it was told. Fixed in emblema (emblema#248); the same frame then reads back `2D3748`. Nothing here had looked at a color on the board: the frames compared with goldens are offscreen ones, and the board's checks counted flips
* **drmkit-capture could not read a ten-bit plane**, so the first capture was the cursor on nothing. It reads the four packed ten-bit formats since drmkit#132

Both are pinned here: drmkit 0.1.5, and emblema at `7d4253a`. Built from the pins, a capture on the Pi 5 has the frame and the cursor, and the cell reads back `2D3748`. Since emblema 0.3.0 reached crates.io with the fix, the pin is that release and no commit.

## Stage 6: the VisionFive 2 (2026-11-17)

The second board: a StarFive VisionFive 2 v1.3B (JH7110, four U74 cores, PowerVR BXE-4-32 through Imagination's Vulkan 1.3 driver), Ubuntu 24.04.3, Linux 6.12.5-starfive, on HDMI. The cross lane's riscv64 kit as it is built, over ssh. seatd and libseat1 installed from Ubuntu's archive, and GDM stopped: its greeter holds the card.

* **The kit's nine checks pass** (M1's exit criteria 2 to 6, docs/m1-exit.md), the first run of any of this on RISC-V hardware
* **The display took no buffer of the renderer's.** Its display controller (card0, VeriSilicon) is a card apart from the GPU (card1), and importing the renderer's dma-buf into it is refused: `importing a buffer: Function not implemented`. The Pi 5's two are one driver's. fl-raster now tries the renderer's own ring and, where that is refused, emblema's ring of buffers GBM allocates on the display's card (`DrmScanoutTarget::with_gbm`), which the renderer imports and draws into. The display says which card is its own (`ScanoutOutput::device_fd`). libgbm is opened when that ring is asked for and not linked
* **`run-app --kms --mode WxH[@HZ]`**: one of the display's own modes by its size, at the rate nearest the one said or its fastest. The monitor's preferred mode is 3840x2160 at 30 Hz, and there `ticker` is shown a frame in two or three blanks (99 flips over 226). `run.sh --mode` passes it on
* **At 1280x720 and 60 Hz: 299 flips over 298 blanks, none missed**, three runs, and the kit's display check passes: 10 passed, 0 failed
* **Looked at on the monitor**, by a person: `ticker`'s red to blue, as it should be
* **The CPU's governor decides that.** As the image boots it is `ondemand`, the cores at 750 MHz of 1,500 while a frame a blank is all they are asked for, and a frame in twelve then took two blanks: 299 flips over 324, at even spacing. With `performance` none does. A board's image is the place for that setting, not the engine

Found on the way:

* **A switch of terminals under an app that was drawing nothing did not come back.** Ctrl+Alt+F2 and Ctrl+Alt+F1 with `cursors` on the display: the seat resumed and the screen stayed the console's. The display took its card again only when a frame was next shown, and noticed the switch only if it had looked during it; an app at rest shows none. The session's coming back now has the last frame drawn again (`Seat::on_resume`, `RasterThread::redrawer`), and the display knows a switch it did not watch by the session's count. The Pi 5's switch had been made under `keys`, where the next key drew. Seen on the Pi 5 since, the display read back after the switch: `cursors`, as before it
* The card's HDMI read no EDID until the board was started again: `EDID has corrupt header`, the connector connected with no mode, two cables alike. fl-dart said so (`no connector of any card has a display with a mode`). After a reboot it lists sixteen
* cross.yml's riscv64 leg failed once on main, qemu-user crashing in its own code in the one test that ran fl-dart without the harness's rerun

### Its devices

A keyboard (HP) and a mouse (Logitech, by its receiver) plugged in, a person at the board, each app shown for a minute or two over ssh at 1280x720. The board's kernel has no `/dev/uinput`, so nothing here is synthetic.

* **Keys**: `keys` heard 243 key downs in one run and 155 in another, each with its physical key, logical key and character, two held at once among them
* **Mouse**: `pointer` heard the mouse added, 2,019 hovers, 11 downs and ups, drags; and in a later run 209 wheel notches, both ways, each a scroll of 53 as on the Pi 5
* **Cursors**: `cursors`, all 35 cells entered; the cursor changes in each, `wait` and `progress` animate
* **A capture**: Print Screen with `--capture` wrote the display's frame with the cursor on it, read back through GBM's buffers, the cells `2D3748` and `4A5568` as drawn
* **A switch of terminals**: Ctrl+Alt+F2 and Ctrl+Alt+F1, twice, the grid back each time and captured after (above, found on the way)
* **`counter`**, the first app with text and Material's widgets on this board: drawn right, the count going up by the click. Its 199 flips over 620 blanks were read as about 20 frames a second, which they are not: the blanks between clicks, when it draws nothing, are counted too (its frame, below). Its first clicks were not answered while the ripple's shaders compiled

The cursor, on this display controller:

* **Near the top and left edges it broke up**, and came back in the middle; the right and bottom edges were clean. There the cursor's image is redrawn on every move, shifted inside its buffer so the plane need not start off the screen, and on the legacy calls it was redrawn into the one buffer being scanned out. Fixed in drmkit 0.1.6 (drmkit#134), pinned here: a second buffer, drawn into and then set. With that, looked at again: clean at both edges, the capture of it in the corner clean
* **When it first appears it is sometimes noise: open, and not fixed here.** A mouse nudged once and left shows a scrambled cursor that stays until the mouse is moved further; a larger movement clears it. What was found, each with a person at the board:
  * The cursor's buffer is right while the monitor shows noise: every capture taken at that moment has a clean arrow, in place
  * It is not every run. The same build, app and nudge gave scrambled, clean, scrambled in three runs
  * It is in `pointer`, which asks for no cursor shape and draws no frame as the mouse moves, and in `counter`
  * Waiting for the app's first frame before touching the mouse does not avoid it
  * Telling the card the cursor again at the same place, a tick and two ticks later, did not clear it; nor did moving it a pixel aside and back
  * Walking 24 MB on each core every ten seconds, to put the caches out to memory, did not clear it in fifteen seconds with the mouse untouched
  * The Pi 5 has not shown it

  It may be the board's display driver, the vendor's for this kernel (6.12.5-starfive), and nothing to be fixed from here; neither change tried is kept. Tracked as #29
* `counter`'s button keeps the arrow: the framework asks for `basic` over it, off the web, as a script's mouse shows here too

Not done on this board: latency as a number (the longest alone is printed, and `counter`'s is its first frame's shaders, 1.9 s), hotplug, a touch screen. `run.sh --input` was not run as such; its two checks are the keys and the pointer downs above.

### Its frame

`run-app --times` on the board, 1280x720 at 60 Hz, emblema 0.3.0, the median of a run's last frames:

| app | recorded | drawn | frames in blanks |
|---|---|---|---|
| ticker | 0.08 ms | 12.0 ms | 299 in 298 |
| text_churn | 6.2 ms | 12.5 ms | 299 in 340 |
| counter, tapped six times by a script | 1.8 ms | 13.7 ms | its 120 frames each begun a blank after the last |

* `counter` was not at 20 frames a second: tapped by a script, every frame of its ripples is begun at the blank after the one before. Its frame is 15.5 ms of a 16.7 ms period, which a mouse moving, or anything else, takes it past; text_churn's 18.7 ms misses a blank in eight
* The draw is nearly the same whatever is drawn. Taken apart inside emblema (a build with clocks in it, not kept), ticker's 12 ms: 2.3 ms making the pass's multisample and stencil buffers, 1.7 recording commands, 5.5 waiting for the GPU, 2.1 freeing. At one sample, 0.1 making and 0.9 freeing. So a third of it is two buffers made and freed every frame, and the multisampling itself costs the GPU under a millisecond
* emblema keeping the two between submissions (jwinarske/emblema#251), the same runs:

| app | drawn, before | after | frames in blanks |
|---|---|---|---|
| ticker | 12.0 ms | 8.8 ms | 299 in 298 |
| text_churn | 12.5 ms | 9.2 ms | 299 in 298 |
| counter, tapped | 13.7 ms | 9.4 ms | as before |

* text_churn holds 60 Hz with it, and `counter`'s frame is 11.2 ms. Released as emblema 0.3.1 and pinned here
* What was left of the 9 ms: 5.5 to 6 the GPU's, waited for on the raster thread; 1.8 recording commands and 0.9 freeing the frame's vertex, index and material buffers and its descriptor pools, made anew each frame as well
* The GPU's part taken off the thread (jwinarske/emblema#254, emblema 0.3.2): the frame is submitted for a fence, which goes with the commit as the plane's `IN_FENCE_FD`, and the kernel waits on it before the flip. The same runs, the thread's time between a scene and its commit:

| app | recorded | buffer | submitted | all, before | after | frames in blanks |
|---|---|---|---|---|---|---|
| ticker | 0.08 ms | 1.2 ms | 2.2 ms | 9.0 ms | 3.5 ms | 299 in 298 |
| text_churn | 6.1 ms | 1.1 ms | 2.3 ms | 15.4 ms | 9.5 ms | 299 in 298 |
| counter, tapped | 1.8 ms | 1.2 ms | 2.3 ms | 11.2 ms | 5.3 ms | as before |

* The buffer's 1.1 ms was nothing before: it is the frame before retired, its buffers freed, where the next buffer is picked up. The GPU's 5.5 ms is as long as it was, and runs while the thread records the next frame; a frame is on the screen no sooner for a thread that waits less, but the thread has 13 ms of a 16.7 ms period for recording a scene where it had 7.5 (no scene that long has been tried)
* `fence_waits=1` in each run, the commit that sets the mode: the kernel waited for every other. On the Pi 5 as well, where a frame's submission is 0.1 ms
* What the display shows on that path, read back (2026-11-18): `--capture` taken ten times in a run of text_churn, Print Screen written to the keyboard's event node, and each compared with the same run's frames drawn offscreen on the board. Every capture is one of them, 1,800 to 2,400 of its 921,600 pixels differing and none by more than one level in 255; text_churn puts new glyphs in the atlas as it goes, so a frame drawn from an atlas being written would show there. `counter`, Tab and Enter written to the same node, counts to 3, captured in a ripple and at rest. This is the buffer the CRTC is showing and not the panel: a flip made before its fence signaled would not show in it
* The panel, looked at the same day: text_churn for 30 seconds (1,799 frames in 1,798 blanks), no tearing; `counter` clicked to 94 with a real mouse; Ctrl+Alt+F2 and back twice, by hand and by keys written to the node, the frame whole each time and a run of 600 frames finishing after it; the keyboard and the mouse unplugged and plugged in again, both working after. The cursor was noise when it first appeared and took a while to come right, as before (#29); twelve captures taken then show the arrow as it should be
* What is drawn for the first time is slow there, and it shows: the first time the mouse is over `counter`'s button nothing changes and a click seems not to take, and after the tooltip has come up everything answers. `--times` with a script that only moves the mouse onto the button: 5 frames, the median's draw 645 ms and the longest 1,049; six taps, one frame of 867 ms and the first tap's frame 0.8 s late, the rest 2.3 ms. The same with emblema 0.3.1 (872 ms), so not from frames not being waited for. Each is a pipeline the driver builds at its first use, the highlight's, the tooltip's, the ripple's; `fl-dart prepare` takes 1,754 ms there every time it is run, so IMG's driver keeps nothing between runs as Mesa's does. A click written to the mouse's node in that time is counted, late
* The pipelines kept on disk between runs (jwinarske/emblema#257, emblema 0.3.3): emblema builds each through a pipeline cache and hands over the driver's bytes, and `run-app --kms` and `prepare` keep them in `fl-engine/pipelines` under `$XDG_CACHE_HOME` or `~/.cache`, written when the raster thread has had nothing to draw for a second after a pipeline was built and when it finishes, owner-only and by rename. IMG's driver takes them back. The mouse moved onto the button and two taps, by a script: the longest draw 792 ms on a first run and 11 on the two after, the latest frame 859 ms and then 151 and 145, 32 frames drawn and then 56, the ripple's and the highlight's being drawn now. `prepare` 1,577 ms and then 335; the file 591 kB. A first run on a board still compiles, and `prepare` at an image's build covers only source-over (until emblema 0.3.4, below). What is left in the longest frame is 130 to 170 ms recording one scene
* That scene is the app's first, and no input's: a script of `idle` alone records it in 138 ms, its glyphs made then. The 145 to 151 ms above is a script's first event arriving while that frame is still being drawn, the script's `idle` being the UI thread's and not the raster thread's. With `wait 600` ahead of the same hover and taps, the latest of the three frames input asked for is 18.2 ms after its event in each of two runs, 63 frames drawn: the board's latency figure, under two frames as the soak holds it
* That scene's recording, taken apart (2026-11-19; offscreen at 1280x720, the board's display unplugged that day, so the display's 138 ms was not taken again). 44.6 ms, and 40.5 of it one call: the glyph atlas's image written for the first time, a megabyte. The same write again is 2.4 ms, and other sizes cost by their size (0.27 ms at 64 by 64, 8.2 at 2048), so the 38 ms is the device's first image write and not this one's bytes. Rasterizing the scene's 51 glyphs is 1.8 ms. The raster thread now makes the atlas's image and writes it empty when it starts, while the app does; a failure there is left for the frame's own write to report. The first frame's recording after: 6.8 to 7.1 ms in three runs. `ticker`'s worst recording 42.4 ms before and 0.09 after, `text_churn`'s 46.7 and 51.6 before and 10.2 and 10.6 after, its median 8.2 either way
* A scaler, the font read and its hinting set up for a size, was made for every run of every frame and used only for a glyph not yet in the atlas: 1.2 ms of the counter's 1.4 ms pass over a scene with nothing new in it. It is made now for a run with a glyph to rasterize. `text_churn` rasterizes in every frame and is as it was
* The same on the display (2026-11-19, the monitor back; 1280x720, pipelines kept, the build before #51 and #51's in turn). The first scene's recording: `counter` 160, 142 and 142 ms before, 6.8, 6.8 and 6.8 after; `material` 151 ms and 0.25; `text_churn` 144 ms and 8.4. The first image write costs more on the display than off it, about 135 ms where it was 38, and why was not looked into: it is out of the first frame either way. The earlier `text_churn` and `ticker` figures of 300 frames never showed it, a stage's worst being of its last frames only
* The scaler's part, in frames after the first: `counter`'s median recording over the 63 frames of a hover and two taps 1.95 ms before and 0.69 after, twice each. The latest input frame 18.2 ms either way; `text_churn` over 300 frames 8.21 and 8.24 ms, `ticker` 0.085 and 0.082, no flip missed in any
* What the first image write was (jwinarske/emblema#263, emblema 0.3.5; 2026-11-19). emblema takes device memory from the driver in blocks, 256 MiB for images and 64 MiB for what the host writes through, and this board's driver backs a block with pages when it is made: `vkAllocateMemory` there is about 0.45 ms a megabyte, 256 MiB in 94 to 125 ms, 64 MiB in 31, 4 MiB in 2. The atlas's image was the display's first, so it made the 256 MiB block, 100 ms, and its write the 64 MiB one, 38; offscreen the surface had made the first already, which is the 38 ms seen there. 320 MiB was held for a counter using 54. No size given would have taken: gpu-allocator 0.27.0 kept its defaults whatever it was handed. emblema 0.3.5 is on 0.28, which keeps them, and has `Context::with_memory`
* A device's blocks are sized by its target now: eighteen of its RGBA frames to the next 4 MiB, 16 MiB at least and 256 at most, so 64 MiB at 1280x720 and 144 at 1920x1080; 8 MiB for the host's. The counter's 54 MiB is fifteen frames, the pass's multisample and stencil buffers and its layers. With a probe build and the sizes from the environment: the start-up image and write 14 ms at 64 and 8 MiB where they were 142, 75 MiB held where 320 was. At 32 MiB or less a second block was made during a hover's frame, whose draw then took 24 to 42 ms where the longest is 11.8; at 64 none is
* The merged sizes on the display, the build before and this one in turn. Memory available on the board while `text_churn` runs is down 343, 363, 360 and 357 MB before and 125, 86, 98 and 108 MB with it, the process's own 86 MB either way. `counter` from start to exit with a script of `idle`, six runs each: 852 to 899 ms, 884 on average, before, and 849 to 867, 858, after. A hover and two taps: the longest draw 11.7 and 12.6 ms before, 11.8 and 11.6 after, the latest input frame 18.2 and 18.2 then 18.3 and 18.1. `text_churn`'s recording over 300 frames 8.17 ms and 8.19, `ticker`'s 0.078 either way, no flip missed
* From the merged build's kit, the file removed first (2026-11-18): the longest draw 1,063 ms, then 11 and 11; the latest frame 1,076 ms, then 151 and 145; 32 frames, then 56 and 56 in 54 blanks. By hand with a real mouse and the file full: the first hover and the first click answer
* A board's first run, prepared (jwinarske/emblema#260, emblema 0.3.4; 2026-11-18). `prepare` had not helped a display's run there at all: the file after it grew by nearly a whole run's pipelines in the run. Three things were missing, found by listing the pipelines `counter` builds. A clip's mask was prepared source-over and a canvas draws it with `Src`, so the two prepared were never used. The first frame, the hover and the click also draw with the shader's shape variant (rounded rectangles, shadows) and its image variant (a layer composited), and only the core one was prepared. And a pipeline is built for its target's format, IMG's driver keeping it by that: the display there is ten-bit, and what was prepared was the offscreen target's. `prepare` now builds the three variants for the offscreen target's format and a display's two, 48 pipelines. With the file removed and `prepare` run, the same hover and two taps: the longest draw 11.5 ms, the latest frame 18.2 ms after its event, 63 frames, the file no larger after the run; it was 755 ms, 450 ms and 32 frames with the eight-bit formats alone, and 830 ms, 843 ms and 32 with `prepare` as it was. `prepare` there takes 13.9 s with nothing kept, 1.5 before for its eight, and a third of a second once the file has them; on a Pi 5 it has not been timed since
* A key held as its keyboard goes, `apps/keys` on the display and its log timed: `t` down, the keyboard pulled 1.7 s later and `key up` with nothing left pressed, nothing for 4.1 s, then `t` again from the keyboard plugged back in. A mouse's button held as it goes has not been seen the same way
* The frame before is waited for in two cases, both before anything is written: a glyph the atlas has not got, since the page is written in place, and an image destroyed, since the frame may be drawn from it. A screen of text that does not change waits for neither

## The binary's size (2026-11-18)

Not in the brief: asked for, and measured on the boards the milestone was.

* A release build is built for size (#41): fat LTO, one codegen unit, `opt-level = "s"`, stripped. `fl-dart` on x86_64 from 15.0 MB (12.5 MB of it stripped) to 10.37 MB; on aarch64 14.7 MB to 9.71, `fl-dart-kms` 15.7 MB to 10.17; on riscv64 13.9 MB to 9.11 and 14.9 MB to 9.55. `"z"` is 0.1 MB less and `panic = "abort"` 0.2 MB, neither taken, a panic at the embedder's boundary being caught
* What it costs on the VisionFive 2, `fl-dart-kms` before and after, 300 frames at 1280x720, two runs each, no blank missed in any: `text_churn`'s scene recorded in 7.85 and 7.91 ms before, 8.19 and 8.22 after, about 4% more; its build 1.26 to 1.41 ms either way, its draw 2.27 to 2.28; `ticker`'s scene 0.080 to 0.086 ms; `counter`'s latest input frame 17.9 ms and 18.1
* The JPEG, GIF and WebP decoders are features (#42), PNG always read: 111, 37 and 149 kB on x86_64, 10.07 MB with none. An image in a format left out is an exception that names it
* The VM without dart:io's secure sockets (#43, #44), the SDK's `dart_disable_secure_socket` and so no BoringSSL, as the prebuilt `aot-nossl` and the feature `no-secure-sockets`: 8.75 MB on x86_64, 8.45 MB with the decoders out too. `SecureSocket.connect`, `SecurityContext` and an `https` URL through `HttpClient` throw `ArgumentError`, "Secure Sockets unsupported on this platform"; a plain socket connects as before. `SecureSocket.connect` has connected its plain socket by then and the SDK does not close it: a script that tries does not exit
* The board kit on that VM (#45), `fl-dart-board-kit-<target>-nossl`, from the cross lane:

| | default | `nossl` |
|---|---|---|
| aarch64 `fl-dart` | 9.71 MB | 8.33 MB |
| aarch64 `fl-dart-kms` | 10.17 MB | 8.72 MB |
| riscv64 `fl-dart` | 9.11 MB | 7.92 MB |
| riscv64 `fl-dart-kms` | 9.55 MB | 8.35 MB |

* On the VisionFive 2, the riscv64 `nossl` kit, `./run.sh --mode 1280x720`: 10 passed, none failed or skipped, the display's among them, 300 frames with 299 flips and no blank missed. No BoringSSL string in `fl-dart-kms`, 178 in the default one. The TLS probe there as on x86_64: `ArgumentError` from `SecurityContext` and from `SecureSocket.connect`, and after the second the script does not exit
* On the Pi 5 (2026-11-19; the kits main's cross lane built at b57bc0d, the board V3D 7.1.7.0 on Mesa 26.2.2). The default kit's `run.sh`: 10 passed, the display's among them, 300 frames at 1280x1440 with 299 flips and no blank missed. The `nossl` kit's: 9 passed and the display's skipped, another session's compositor holding the seat by then; no BoringSSL string in its `fl-dart-kms`, 173 in the default one; 8.33 and 8.72 MB where the default's are 9.71 and 10.17. The TLS probe, `notls.dart` for arm64: the same `ArgumentError` from `SecureSocket.connect`, an `https` URL and a certificate put in a context, each socket closed, exit 0
* `fl-dart prepare` there, with nothing kept: 827 ms, and 65 and 66 ms with the file it left, 2.3 MB. Offscreen at 1280x720: `counter`'s first scene recorded in 1.05 to 1.19 ms, `text_churn`'s recording 1.39 ms at the median and 1.95 at most over 120 frames, its build 0.43 ms. Not taken there: any of it on the display, what the memory's blocks hold, and a first run's frames with and without `prepare`
* The Pi 5's display, later that day with the seat free. The `nossl` kit's `run.sh`: 10 passed, 300 frames at 1280x1440 with 299 flips and no blank missed
* What #54's blocks did there, and emblema 0.3.6 (jwinarske/emblema#266). V3D has one kind of memory, mapped by the host, and gpu-allocator sizes a block by its memory type: every image on that board came from the 8 MiB host blocks, and one larger had an allocation of its own in the frame that first drew with it. `counter`'s first draw took 9.4 and 9.6 ms at 1280x1440 and 5.0 and 5.2 at 1280x720, where the build before #54 took 0.96 and 0.74. 0.3.6 makes every block the larger size on such a driver. With it, the build before #54, #54's and this one in turn, twice: the first draw 0.96 and 0.94 ms at 1280x1440, 0.71 and 0.73 at 1280x720. The longest draw of a hover and two taps 0.89 and 1.0 ms, where #54's was 9.6 and 18.9 and the one before 20.8 and 21.2: the layers a hover makes needed a new 64 MiB block then and are in the one made at the start now. The latest input frame 17.1 ms in all three, 17.3 before #54. `text_churn` over 300 frames, its recording 1.06 ms at the median where it was 1.38 before #54, no flip missed
* What the blocks hold on the Pi 5 (2026-11-19, later). The system's available memory says nothing there, 79 to 223 MB from sample to sample; the process's own account does, V3D's `drm-total-memory` and `drm-resident-memory` in `/proc/<pid>/fdinfo`, read three seconds into a run on the display. The two are equal in every run: V3D backs what it is asked for when it is asked, as the VisionFive 2's driver does. The build before #54, #54's, and this one:

| | before #54 | #54 | emblema 0.3.6 |
|---|---|---|---|
| `text_churn`, 1280x1440 | 88 MiB | 119 MiB | 154 MiB |
| `counter` after a hover and two taps, 1280x1440 | 218 MiB | 203 MiB | 152 MiB |
| `text_churn`, 1280x720 | 78 MiB | 64 MiB | 78 MiB |
| `counter` after a hover and two taps, 1280x720 | 77 MiB | 77 MiB | 77 MiB |

* Read with the display's ring in it, three buffers the driver counts too, 21 MiB at 1280x1440. Before #54 every block there was a 64 MiB one, the board having one kind of memory: one for `text_churn`, three once a hover had made its layers, the second and third made in the frames that needed them, which is the 21 ms draw. Now one block of 128 MiB holds either. So an app with no layers holds 66 MiB more than it did at that size and one with them 66 MiB less, and at 1280x720, where the block is the 64 MiB it always was, nothing changes. Eighteen frames is more than `text_churn` uses, twelve as measured later (86 MiB, from emblema's own count, jwinarske/emblema#269; the nine said here first was a guess from the totals); a smaller block would be made up for in a frame, by a second one
* A device's block is sized by what the app needed in a run before (emblema 0.3.7, jwinarske/emblema#269, which counts what a context allocates and the most it has at once). A run on a display keeps that figure in `fl-engine/memory` beside the pipelines, a line for the app, by its file's path, length and time of writing, at the display's size, and only ever raised; the next run's block is that and one frame more, and a run with no line is sized as before, eighteen frames. `--times` prints a `memory:` line, the block, the most of each kind allocated, and whether the block was the app's figure or the target's. On the Pi 5 at 1280x1440, three runs of each with the file removed first: `text_churn` 86 MiB at the most, a block of 128 MiB on the first run and 96 on the two after, the process's GPU memory 154 MiB then 122; `counter` after a hover and two taps 114 MiB, 128 then 124, 152 then 148. No second block in any, the longest draw under 1 ms in every run and the latest input frame 17.1 ms. So 32 MiB for an app with no layers at that size and 4 for one with them; nine frames, which was the guess, would have been 66
* The remembered block at other sizes (2026-11-20), on the Pi 5's HDMI at four of its modes, three runs of each app at each with the file removed first, the build #59 merged. The block on the first run, the target's, then on the two after, the app's; the most each allocated; the process's GPU memory then and after:

| | `text_churn` | | | `counter` after a hover and two taps | | |
|---|---|---|---|---|---|---|
| | block | most | held | block | most | held |
| 640x480 | 24, 20 MiB | 18 MiB | 28, 24 MiB | 24, 24 MiB | 22 MiB | 33, 33 MiB |
| 1024x768 | 56, 44 | 40 | 67, 55 | 56, 56 | 52 | 67, 67 |
| 1280x1440 (above) | 128, 96 | 86 | 154, 122 | 128, 124 | 114 | 152, 148 |
| 1600x900 | 100, 76 | 71 | 121, 97 | 100, 100 | 93 | 119, 119 |
| 3840x2160 at 30 Hz | 256, 256 | 386 | 626, 626 | 256, 256 | 769 | 1138, 1138 |

* Up to 1600x900 it does what it did at 1280x1440: an app with no layers holds 4 to 32 MiB less from its second run, and one with them about the same, its layers using what eighteen frames leave; no second block, the longest draw under 0.9 ms, the latest input frame 17.0 to 17.3 ms
* At 3840x2160 the block is held to 256 MiB, eight frames of 32 MiB, and a remembered figure larger than that changes nothing. `text_churn` needs twelve frames and makes a second block, in a frame not timed or at no cost to a draw: its longest is 0.18 ms. `counter`'s hover needs twenty-four and makes three more, one in each frame its layers first need it: 16 ms at the median of a hover's nine frames and 97 ms at the longest, in each of three runs, the latest input frame 34 ms, one blank at 30 Hz. The process holds 1.1 GB of the board's 8. A block the size of the remembered figure would be made at the start instead
* The DSI panel was not attached, though its connector reads connected; nothing measured there
* The VisionFive 2 with 0.3.6 is not measured, off the network that day; it has memory of the device's own, and the code takes the path it took
* The board restarted once in that session, between the default kit's run ending and the next command, twelve minutes off the network. Looked into after: the firmware's reset status reads 0x1000, a power-on and not the watchdog's reset or a reboot; the log stops with no kernel message, and a file copied to the board seconds before came back with no data in it, so it stopped at once and not by shutting down; the hardware watchdog, a minute's, would have had it back in two. Four of the board's last five boots ended that way, without a shutdown, on the 16th, twice on the 17th and this one. Both kits run back to back three more times, with the same headless compositor of another session's running as then, and some hundred runs on the display since: nothing. So its power was taken and given back, by what is not known here, and nothing found says a run of ours did it
* The socket a refused TLS connection had opened is closed (#48): the SDK's stubs threw at the default `SecurityContext`, before the `try` in dart:io whose `catch` closes the socket, and the ones on the way to `SecureSocket_Connect` are quiet now, so `Connect` throws inside it. On x86_64 a program that tries `SecureSocket.connect` and an `https` URL against its own plain server gets the same `ArgumentError` from each, sees each socket close, and ends; before, it did not end. A `SecurityContext` can be made, and nothing put in it. In the L2 suite on that VM, `a_refused_tls_connection_closes_its_socket`
* The dictionaries Thai, Lao, Khmer and Burmese break by, fl-text's `dictionary`, are a feature of `fl-dart` too, on by default: 1.10 MB on x86_64, the largest single thing a build can leave out. Without it `apps/breaks` differs in those four scripts' lines alone, a run of one being one word; 10.37 MB by default, 9.26 without the dictionaries, 8.97 with the decoders out too, 7.35 with secure sockets out as well
* Sharing Unicode data between the VM's ICU and the text stack was looked at and left: the text stack's own tables are about 90 kB (unicode-linebreak 21, unicode-bidi 16, unicode-segmentation 4, the shaper's 45), the VM's ICU 674 kB and needed by the VM whatever the text stack does, and it carries no break rules or dictionaries to share the other way. Line breaking, bidi and segmentation rewritten over ICU's properties would save under 1% of the binary
* What is left in the binary, by symbol size in the x86_64 build before any of this: fonts, shaping and Unicode tables 2.0 MB, the VM and dart:io 1.15, Rust's std 1.05, ICU in the prebuilt 0.7, fl-engine's crates 0.6, emblema 0.3. The VM's ICU and the text stack's tables are Unicode data twice

## Decisions taken 2026-11-16

D1 (devices and a script, no window), D4 (clipboard in the process), D5 and D6 (no system IME), each with what the stock Linux embedder does in its place (brief §13). D5 as taken: no recordings from the stock embedder; the engine's unit tests ported for each layer, and the same scripted input given to `flutter_tester` and to fl-dart end to end.
