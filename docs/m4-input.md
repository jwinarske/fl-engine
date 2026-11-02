# M4: input

> Brief, 2026-11-02. Draft: the decisions in §13 are proposals until taken. Findings from bring-up
> go in `docs/m4-status.md`, newer where the two disagree.

**`flutter create`'s counter app, unmodified, counts the taps it is given, and a `TextField` takes typed text, a composed character, a selection made by dragging, and copy and paste; each frame matches the stock engine's golden within a recorded tolerance. Pointer, key and text-input events reach the framework as the engine's Linux embedder delivers them, from Linux input devices on the boards and from a script in CI, in pure Rust.**

Sources, read at the pin (Flutter 3.47.8, engine `cf636b1d`, Dart `416e33b0`): engine `lib/ui/window` (pointer_data, pointer_data_packet, pointer_data_packet_converter, key_data, key_data_packet), `lib/ui/hooks.dart` (`_dispatchPointerDataPacket`, `_dispatchPlatformMessage`), `lib/ui/platform_dispatcher.dart` (`flutter/keydata`); the Linux embedder `shell/platform/linux`: fl_engine, fl_pointer_manager, fl_scrolling_manager, fl_touch_manager, fl_keyboard_manager, fl_key_embedder_responder, fl_key_channel_responder, fl_key_mapping.g.cc, fl_keyboard_layout, fl_keyboard_channel, fl_text_input_handler, fl_text_input_channel, fl_platform_channel, fl_mouse_cursor_channel. Framework `gestures/converter.dart`, `services/hardware_keyboard.dart`, `services/raw_keyboard_linux.dart`, `services/text_input.dart`, `widgets/editable_text.dart`.

---

## 1. What M4 Is and Is Not

**Is:**
* pointer: mouse, touch and scroll, as `PointerDataPacket`s through `_dispatchPointerDataPacket`, with the engine's packet converter's synthesized events (add, hover and move to where the pointer is, cancel and remove)
* keys: `KeyData` on `flutter/keydata` and the legacy `RawKeyEvent` on `flutter/keyevent`, both, as the Linux embedder sends them; `flutter/keyboard`'s `getKeyboardState`
* text input: the `flutter/textinput` channel, editing state, `TextInputAction`s, composing (the IME's preedit) and its region
* selection: dragging, double-click and shift-click select; the framework paints selection and caret with M3's `getBoxesForRange`, so painting needs nothing new
* the clipboard: `Clipboard.setData`, `getData`, `hasStrings` on `flutter/platform`, within the process
* platform messages from the engine to the framework, and the framework's replies to them: nothing sends one today
* input sources: Linux evdev devices on the boards, a scripted source in CI and for tests

**Is not:**
* a Wayland or X11 window (M2's D1 stands); input on a desktop comes with one
* a system IME (IBus, Fcitx) or an on-screen keyboard: composing is driven by the scripted source and by the dead keys and compose sequences of the keymap (§5)
* drawing a mouse cursor: `flutter/mousecursor` is answered, and the cursor plane is a board-stage question (§14)
* accessibility and semantics, gamepads, stylus pressure and tilt beyond what the packet carries, drag and drop from outside the app
* a system clipboard shared with other processes
* KMS on the boards beyond what M2's stage 6 brings

## 2. The Contract Being Implemented

**Pointer.** The embedder sends `PointerData` records, 36 fields of 8 bytes each (`kPointerDataFieldCount`), packed into a `PointerDataPacket` and delivered to `_dispatchPointerDataPacket(ByteData)`. Before that, the engine's `PointerDataPacketConverter` makes the stream well-formed: an `add` for a pointer not yet added, before its hover, down, pan-zoom or signal; a `hover` or `move` to the event's position when it is not where the pointer was; a `cancel` and `remove` when a pointer that is down is added again or removed; a `remove` for a pointer never added is ignored, as is an event for an unknown view. Scroll is a pointer signal (`kScroll`) with deltas; trackpad pan-zoom is its own phase. Positions are physical pixels; the framework divides by the view's ratio.

**Keys.** One key event becomes:
* a `KeyData` packet on `flutter/keydata`, its physical key a USB HID usage and its logical key the layout's, its type down, up or repeat, and its `synthesized` flag for the key states the embedder corrects (fl_key_embedder_responder)
* a JSON message on `flutter/keyevent` in the GTK shape `RawKeyEventDataLinux` reads: `keymap: linux`, `toolkit: gtk`, `type` (`keydown`, `keyup`), `keyCode`, `scanCode`, `modifiers`, `unicodeScalarValues`, `specifiedLogicalKey` (fl_key_event_channel)

The embedder waits for both answers; the event is handled if either says so (fl_keyboard_manager), and an unhandled key goes to text input, then back to the toolkit (fl_view). Physical keys come from the generated table `fl_key_mapping.g.cc`, xkb keycode to HID usage; on evdev the xkb keycode is the evdev code plus 8, so the same table serves. `flutter/keyboard`'s `getKeyboardState` answers the keys held down when the framework starts listening.

**Text input.** `flutter/textinput`, JSON method codec. The framework sends `TextInput.setClient` (client id, configuration), `setEditingState`, `show`, `hide`, `clearClient`, `updateConfig`, and the geometry calls (`setEditableSizeAndTransform`, `setMarkedTextRect`, `setStyle`), which an IME positions its window by. The embedder sends `TextInputClient.updateEditingState` (text, selection base and extent, composing base and extent, in UTF-16 code units), or the `WithDeltas` form when the configuration asks, and `performAction` (`TextInputAction.done`, `newline`, ...). The embedder edits the text itself for a key the framework did not handle: a character inserted, backspace and delete, arrows within the composing text (fl_text_input_handler).

**Clipboard.** `flutter/platform`, JSON method codec: `Clipboard.setData` with `text`, `getData` with `text/plain`, `hasStrings`.

## 3. Engine-to-Framework Messages

M2 and M3 answer messages the framework sends. M4 is the first to send one: the engine calls `_dispatchPlatformMessage(name, data, responseId)` and the framework answers with `PlatformDispatcher_RespondToPlatformMessage(responseId, data)`. Pointer packets go through their own hook; keys and text input through this path. fl-shell owns the response ids: a message's continuation is kept by id and run when the framework responds, or with null if the isolate shuts down first, so a key event never waits forever.

## 4. Input Sources

* **evdev** (boards): `/dev/input/event*`, read with `rustix` (raw syscalls, no C), opened non-blocking on an input thread; `EVIOCGRAB` so the console does not also read the keys; devices found at start and on hotplug by watching `/dev/input` with inotify. Relative axes and buttons are a mouse, absolute multitouch slots a touch screen, `EV_KEY` key codes a keyboard. `SYN_REPORT` ends a frame of events; `SYN_DROPPED` resynchronizes the device's state
* **scripted** (CI, tests): `--input <file>`, one event per line at a frame number or time: `tap x y`, `down`/`move`/`up`, `scroll dx dy`, `key <name> down|up`, `text "..."`, `compose "..." "..."`. Lines become the same internal events evdev produces, so everything after the source is the board's path
* Both feed one queue to the UI side, drained between loop steps as replies are; arriving input wakes the loop and asks for a frame as the engine's does

## 5. Keyboard Layout and Text

The logical key and the character a key types depend on the layout. The Linux embedder takes both from GDK, which has them from xkbcommon, a C library. fl-engine needs a keymap in pure Rust:
* **logical keys and text**: an XKB keymap compiled from the system's rules (RMLVO: `evdev`, `pc105`, `us`, ...), the keysym for each key and modifier state, and the keysym's character. A pure-Rust XKB implementation is checked at stage 3 (`kbvm`); the fallback is a built-in US layout with the keysym tables as data (D3)
* **dead keys and compose**: the system's `Compose` file, so `´` then `e` composes `é`; composing shows as the editing state's composing region, as a preedit does
* **modifiers and lock state**: Shift, Control, Alt, Meta, CapsLock, NumLock tracked by the keymap, and corrected into `KeyData` with synthesized events as fl_key_embedder_responder does

## 6. Threads

* **input thread**: reads devices, keeps device state, turns raw events into pointer records and key events; never touches Dart
* **UI thread**: converts records into packets (the converter's state lives here), dispatches packets, sends key messages and waits on their responses through the loop, edits text input state, answers `flutter/textinput` and `flutter/platform`
* **raster thread**: unchanged

Packets are made on the UI thread at dispatch, as the engine's `PointerDataDispatcher` sends them on the UI task runner, so a frame's moves can be coalesced before they reach Dart.

## 7. Crate Structure

    fl-input       new: input events, the evdev and scripted sources, the input thread;
                   the keymap and compose; no Dart
    fl-ui          pointer packets (PointerData encoding, the converter), KeyData encoding,
                   the text input and clipboard channels, engine-to-framework messages
    fl-shell       the input queue in the loop, response ids, frame requests on input
    fl-dart-cli    run-app --input <file>, --evdev; app-cycle with input for the soak

`rustix` is a new dependency (pure Rust, `linux_raw` backend); the keymap's is decided by D3.

## 8. Test Programs

* `counter`, unmodified: tapped three times, its frame shows 3
* `apps/text_field`: a `TextField` given text, a composed `é`, a selection dragged across a word, a copy and a paste; prints its editing state after each step
* `apps/keys`: records every `KeyEvent` and `RawKeyEvent` the framework sees, logical and physical keys, characters, modifiers, for a script covering letters, Shift, CapsLock, a dead key, repeat, and keys held across focus
* `apps/scroll`: a `ListView` scrolled by wheel, dragged, and flung by touch; prints its offset each frame
* `apps/pointer`: a `Listener` printing every `PointerEvent`, for the converter's synthesized events

## 9. Goldens and References

* **Frames**: `flutter_tester` with Impeller as in M3, the input given by `tester.tap`, `tester.enterText`, `tester.drag` and `sendKeyEvent` at the same points; the frame after the input compared with the recorded tolerance
* **Events**: `flutter_tester` injects input above the engine, so it cannot be the reference for what the engine sends. The reference for `apps/keys` and `apps/pointer` is the stock Linux embedder, a release build run under X11 with `xdotool` replaying the same script, recorded once into `test/*.txt`, as `metrics.txt` was for M3. Recorded on a desktop, outside CI (D5)

## 10. Test Plan

* **Unit**: `PointerData` encoding against the engine's field order; the converter's synthesized events against `pointer_data_packet_converter_unittests.cc`'s cases; `KeyData` encoding; the evdev-to-HID table against `fl_key_mapping.g.cc`; UTF-16 offsets in editing state for text with characters outside the BMP
* **App** (ci.yml `Flutter apps`): the §8 programs from scripts, frames against goldens, event logs against the recorded references
* **Soak**: `app-cycle` with input: a text field typed into for 10,000 frames; pointer churn; memory, handles and the build-time budget held as in M3
* **Sanitizers**: the input thread under TSan; the script parser and the editing-state decoder fuzzed
* **Boards**: a USB keyboard, mouse and touch screen on Pi 5 and VisionFive 2 through evdev, the board kit's input check

## 11. Exit Criteria

1. `counter`, unmodified, counts three scripted taps and matches its golden
2. `text_field` takes text, a composed character, a dragged selection, copy and paste, and matches its golden after each
3. `keys` and `pointer` match the stock Linux embedder's recorded events, synthesized events included
4. `scroll` scrolls by wheel, drag and fling to the stock engine's offsets within a pixel
5. Every message the framework sends on `flutter/textinput`, `flutter/keyboard`, `flutter/platform`'s clipboard and `flutter/mousecursor` is answered; none is left waiting
6. The input thread never touches Dart: input events are `Send`, the converter and editing state are not, each with a compile-fail case
7. Leak, sanitizer and fuzz lanes green with the M4 programs; every limit in §15 has a test that crosses it
8. Still no C toolchain: evdev, the keymap and compose are pure Rust; the no-C++ lane covers them

## 12. Staged Path

1. **Engine-to-framework messages.** Response ids in fl-shell, `_dispatchPlatformMessage`, the reply native, null on shutdown. Exit criterion 5's path
2. **Pointer.** `PointerData` and packets, the converter, the scripted source; `counter` tapped. Exit criterion 1
3. **Keys.** `KeyData` and `flutter/keyevent`, the HID table, the keymap (D3), `getKeyboardState`; `apps/keys`. Exit criterion 3, keys
4. **Text input.** `flutter/textinput`, editing state and deltas, actions, composing, the clipboard; `text_field`. Exit criterion 2
5. **Scroll and touch.** Pointer signals, pan-zoom, multitouch; `scroll`, `pointer`. Exit criteria 3 and 4
6. **evdev.** The input thread, devices, hotplug, grab; the boards. Board check
7. **Lanes.** Soak with input, fuzz targets, TSan, compile-fail cases. Exit criteria 6 to 8

## 13. Decisions (proposed)

* **D1, input sources: evdev and a script, no window.** M2's D1 still rules out a Wayland or X11 client without a C toolchain. The boards read their devices directly, as a KMS kiosk does; CI scripts the same events
* **D2, keys sent both ways.** `KeyData` and `flutter/keyevent`, as the Linux embedder at the pin does: the framework's `HardwareKeyboard` and its legacy `RawKeyboard` both still listen, and an app may use either
* **D3, keymap: a pure-Rust XKB if one holds up, else built-in US.** To check at stage 3: compiling the system keymap, keysyms to characters, and the Compose file, in pure Rust with no build script. If none does, a US layout and US-international dead keys as data, and other layouts deferred
* **D4, clipboard in the process.** No system clipboard exists without a display server; copy and paste work within the app
* **D5, event references recorded once, outside CI.** The stock embedder needs X11 and GTK, which CI's lanes refuse to install; the recording is checked in with the script and toolchain that made it
* **D6, no system IME.** IBus and Fcitx are reached over D-Bus and need a session; composing is tested through the keymap's compose and the script. The `flutter/textinput` side is complete either way, so an IME is a source to add later

## 14. Risks

* **The packet converter's rules are subtle.** Its unit tests are the specification; ported case by case
* **UTF-16.** The framework counts text in UTF-16 code units, fl-text and Rust in UTF-8 bytes. Every offset crossing the channel is converted and checked, and a character outside the BMP is in every editing test
* **Keymap coverage.** If D3 falls back, non-US layouts wait. Each layout is data, not code
* **Event-reference drift.** A recorded reference ages with the pin; a pin bump re-records it (D5)
* **The cursor on KMS.** A mouse on a board with no cursor drawn is unusable for anything but touch-like taps. A cursor plane through emblema's scanout, or a sprite in the frame, is decided at stage 6

## 15. Reliability, Security and Performance

**Security: untrusted input**
* **Device input** (stage 6). Events from a device are bounded: coordinates clamped to the view, slot and key codes checked against their tables, an unknown code dropped, not indexed with
* **Grabs** (stage 6). `EVIOCGRAB` keeps keys away from the console, so Ctrl+Alt+F-keys cannot switch away under a kiosk; a grab that fails is reported, not ignored
* **Permissions** (stage 6). Reading `/dev/input` needs the `input` group or a seat; a device that cannot be opened is named, and the app runs without it
* **Channel payloads** (stages 1, 4). `flutter/textinput` and `flutter/platform` messages are decoded under the M2 decoders' rules: a malformed message is an error reply, never a panic, never an index out of range. UTF-16 offsets outside the text are clamped, as the embedder's are

**Reliability**
* **Every message answered** (stage 1). Each message the engine sends gets its continuation run, by the framework's response or with null at shutdown; each message the framework sends on an M4 channel gets a reply. The soak lane counts persistent handles, as in M3
* **Bounded queue** (stage 2). Input arriving faster than frames is coalesced: moves within a frame merged per device, as the engine's dispatcher allows; keys and buttons never dropped. The queue's length is held to a limit, past which moves are dropped first
* **Key state** (stage 3). Keys held across a focus change, a device unplugged with a key down, a repeat after the release: synthesized up events, as fl_key_embedder_responder makes them, so the framework never thinks a key is still down
* **Hotplug** (stage 6). A device removed mid-gesture ends its pointers with `cancel` and its keys with synthesized ups

**Performance**
* **Latency** (stage 2). Input arriving wakes the loop and asks for a frame at the next vsync; measured from the event's timestamp to the frame's `render`, reported in the soak lane, held to two frames at 60 Hz
* **Typing** (stage 4). Each key in a text field is an editing-state round trip and a relayout; the soak types 10,000 characters and holds the build time to M3's 4 ms budget
* **Boards** (stage 6). Tap-to-frame latency on Pi 5 and VisionFive 2
