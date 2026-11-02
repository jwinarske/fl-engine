# M4: input

> Brief, 2026-11-02. Draft: D2, D3, D7, D8, D9 and D10 in §13 were taken 2026-11-02; the rest
> are proposals until taken. Findings from bring-up go in `docs/m4-status.md`, newer where the two disagree.

**`flutter create`'s counter app, unmodified, counts the taps it is given, and a `TextField` takes typed text, a composed character, a selection made by dragging, and copy and paste; each frame matches the stock engine's golden within a recorded tolerance. Pointer, key and text-input events reach the framework as the engine's Linux embedder delivers them, from the boards' input devices through drmkit and from a script in CI.**

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
* input sources: the boards' devices through drmkit (libinput, under a seat), a scripted source in CI and for tests

**Is not:**
* a Wayland window (M2's D1 stands for now); the desktop is a Wayland backend, later (D8). No X11 in any form (D10)
* a system IME (IBus, Fcitx) or an on-screen keyboard: composing is driven by the scripted source and by the dead keys and compose sequences of the keymap (§5)
* a cursor theme of its own: the cursor is drmkit's (XCursor themes, on the cursor plane), answered on `flutter/mousecursor`
* accessibility and semantics, gamepads, stylus pressure and tilt beyond what the packet carries, drag and drop from outside the app
* a system clipboard shared with other processes
* platforms other than KMS: each is a backend behind the boundary (D8)

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

* **KMS, through drmkit** (boards): `drmkit-input`'s `Seat` reads the devices through libinput and `drmkit-session` holds them through libseat, so devices come and go with a virtual-terminal switch and with hotplug, with no root and no `input` group. `drmkit-input`'s `Keyboard` resolves keys through xkbcommon; its `KeyRepeater` repeats them. libinput handles touchpads, acceleration and device quirks. The same backend owns the display (D9)
* **scripted** (CI, tests): `--input <file>`, one event per line at a frame number or time: `tap x y`, `down`/`move`/`up`, `scroll dx dy`, `key <name> down|up`, `text "..."`, `compose "..." "..."`. Keys go through the same keymap as on the boards; everything after the source is the board's path
* Both feed one queue to the UI side, drained between loop steps as replies are; arriving input wakes the loop and asks for a frame as the engine's does

## 5. Keyboard Layout and Text

The logical key and the character a key types depend on the layout. The Linux embedder takes both from GDK, which has them from xkbcommon. fl-engine takes them from xkbcommon too, through drmkit-input's `Keyboard` (D3):
* **logical keys and text**: an XKB keymap compiled from the system's rules (RMLVO: `evdev`, `pc105`, `us`, ...), the keysym for each key and modifier state, and the keysym's character
* **dead keys and compose**: the system's `Compose` file, so `´` then `e` composes `é`; composing shows as the editing state's composing region, as a preedit does
* **modifiers and lock state**: Shift, Control, Alt, Meta, CapsLock, NumLock tracked by the keymap, and corrected into `KeyData` with synthesized events as fl_key_embedder_responder does, by one tracker every XKB source shares (D7)

## 6. Threads

* **input thread**: dispatches drmkit's `Seat` (or reads the script), keeps device state, turns raw events into pointer records and key events; never touches Dart
* **UI thread**: converts records into packets (the converter's state lives here), dispatches packets, sends key messages and waits on their responses through the loop, edits text input state, answers `flutter/textinput` and `flutter/platform`
* **raster thread**: unchanged

Packets are made on the UI thread at dispatch, as the engine's `PointerDataDispatcher` sends them on the UI task runner, so a frame's moves can be coalesced before they reach Dart.

## 7. Crate Structure

    fl-input           new: the boundary (D8). Events as data, D7's key-state tracker,
                       the editing model, the traits a platform implements (text input,
                       clipboard, cursor). No OS code, no Dart; builds with no backend
    fl-input-script    new: the scripted source
    fl-platform-kms    new: drmkit. Display through emblema's `ScanoutOutput`, session,
                       input, keymap, cursor (D9)
    fl-ui              pointer packets (PointerData encoding, the converter), KeyData
                       encoding, the text input and clipboard channels, engine-to-
                       framework messages; fl-input's types only
    fl-shell           the input queue in the loop, response ids, frame requests on input
    fl-dart-cli        run-app --input <file>, --kms; app-cycle with input for the soak

drmkit is a new dependency (0.1.3, MIT), with its C libraries as they are: libinput and libudev, xkbcommon, libseat. fl-engine's declared Rust floor rises from 1.85 to 1.94.1, drmkit's, which the pinned toolchain already is. Nothing above `fl-input` names a backend crate; a check in the no-vm lane holds it.

## 8. Test Programs

* `counter`, unmodified: tapped three times, its frame shows 3
* `apps/text_field`: a `TextField` given text, a composed `é`, a selection dragged across a word, a copy and a paste; prints its editing state after each step
* `apps/keys`: records every `KeyEvent` and `RawKeyEvent` the framework sees, logical and physical keys, characters, modifiers, for a script covering letters, Shift, CapsLock, a dead key, repeat, and keys held across focus
* `apps/scroll`: a `ListView` scrolled by wheel, dragged, and flung by touch; prints its offset each frame
* `apps/pointer`: a `Listener` printing every `PointerEvent`, for the converter's synthesized events

## 9. Goldens and References

* **Frames**: `flutter_tester` with Impeller as in M3, the input given by `tester.tap`, `tester.enterText`, `tester.drag` and `sendKeyEvent` at the same points; the frame after the input compared with the recorded tolerance
* **Events**: `flutter_tester` injects input above the engine, so it cannot be the reference for what the engine sends. The reference for what the key tracker and the packet converter emit is Flutter's own unit tests, ported: `fl_key_embedder_responder_test.cc` (D7) and `pointer_data_packet_converter_unittests.cc`. End-to-end recordings of `apps/keys` and `apps/pointer` from the stock Linux embedder are D5's question

## 10. Test Plan

* **Unit**: `PointerData` encoding against the engine's field order; the converter's synthesized events against `pointer_data_packet_converter_unittests.cc`'s cases; `KeyData` encoding; the evdev-to-HID table against `fl_key_mapping.g.cc`; UTF-16 offsets in editing state for text with characters outside the BMP
* **App** (ci.yml `Flutter apps`): the §8 programs from scripts, frames against goldens, event logs against the recorded references
* **Soak**: `app-cycle` with input: a text field typed into for 10,000 frames; pointer churn; memory, handles and the build-time budget held as in M3
* **Sanitizers**: the input thread under TSan; the script parser and the editing-state decoder fuzzed
* **Boards**: a USB keyboard, mouse and touch screen on Pi 5 and VisionFive 2 through drmkit, a terminal switch away and back, the board kit's input check

## 11. Exit Criteria

1. `counter`, unmodified, counts three scripted taps and matches its golden
2. `text_field` takes text, a composed character, a dragged selection, copy and paste, and matches its golden after each
3. `keys` and `pointer` match the stock Linux embedder's recorded events, synthesized events included
4. `scroll` scrolls by wheel, drag and fling to the stock engine's offsets within a pixel
5. Every message the framework sends on `flutter/textinput`, `flutter/keyboard`, `flutter/platform`'s clipboard and `flutter/mousecursor` is answered; none is left waiting
6. The input thread never touches Dart: input events are `Send`, the converter and editing state are not, each with a compile-fail case
7. Leak, sanitizer and fuzz lanes green with the M4 programs; every limit in §15 has a test that crosses it
8. Still no C compiler: Cargo builds no C or C++, and the platform's system libraries (libinput, libudev, xkbcommon, libseat) are linked, not built; the no-C++ lane covers the build. The core crates, `fl-input` and `fl-input-script`, link none of them

## 12. Staged Path

1. **Engine-to-framework messages.** Response ids in fl-shell, `_dispatchPlatformMessage`, the reply native, null on shutdown. Exit criterion 5's path
2. **Pointer.** `PointerData` and packets, the converter, the scripted source; `counter` tapped. Exit criterion 1
3. **Keys.** `KeyData` and `flutter/keyevent`, the HID table, the keymap (D3), `getKeyboardState`; `apps/keys`. Exit criterion 3, keys
4. **Text input.** `flutter/textinput`, editing state and deltas, actions, composing, the clipboard; `text_field`. Exit criterion 2
5. **Scroll and touch.** Pointer signals, pan-zoom, multitouch; `scroll`, `pointer`. Exit criteria 3 and 4
6. **KMS on the boards, through drmkit.** M2's stage 6 folded in: display through emblema's `ScanoutOutput` over drmkit, the seat, devices, hotplug, the cursor plane; Pi 5 and VisionFive 2. M2's exit criterion 4 and the board check
7. **Lanes.** Soak with input, fuzz targets, TSan, compile-fail cases. Exit criteria 6 to 8

## 13. Decisions

* **D1, input sources: the boards' devices and a script, no window.** M2's D1 still rules out a Wayland client for now; Wayland is a later backend (D8). The boards read their devices through drmkit, as a KMS kiosk does; CI scripts the same events. Developing without a window means scripts and offscreen frames, or running from a text console on a machine with KMS
* **D2, keys sent both ways** (taken 2026-11-02). `KeyData` and `flutter/keyevent`, as the Linux embedder at the pin does: the framework's `HardwareKeyboard` and its legacy `RawKeyboard` both still listen, and an app may use either. The event is handled if either answer says so; only then does it go to text input. `flutter/keyevent`'s payload is the platform's (`keymap: linux`, `toolkit: gtk` here), supplied by the source; the channel lives in one module, to go when the framework drops `RawKeyEvent`
* **D3, keymap: xkbcommon, through drmkit** (taken 2026-11-02). drmkit-input's `Keyboard`: the system's keymap from RMLVO names, keysyms and their characters, Compose, modifiers and LEDs. `kbvm`, a pure-Rust XKB, was the candidate while the keymap had to be pure Rust; with drmkit's libraries taken as they are, it is not needed. A backend on a platform without xkbcommon brings its own keymap
* **D4, clipboard in the process.** No system clipboard exists without a display server; copy and paste work within the app
* **D5, end-to-end event references** (proposed). Flutter's unit tests are the reference for the tracker and the converter either way (§9). Recordings of the stock Linux embedder would add one end to end: `flutter build linux` (GTK, on Wayland through GDK) under a headless wlroots compositor (cage or sway, `WLR_BACKENDS=headless`), the script replayed through the virtual-keyboard and virtual-pointer protocols (`wtype`, `wlrctl`; to check), every event the framework sees logged and checked in. On Wayland GTK takes its keymap from the compositor as XKB, so its key path is closer to drmkit's than any other reference. Building the stock runner needs clang, CMake and GTK's headers, a C++ toolchain, so it is made outside the regular lanes: by hand, or a manual workflow of its own. Or not at all
* **D6, no system IME.** IBus and Fcitx are reached over D-Bus and need a session; composing is tested through the keymap's compose and the script. The `flutter/textinput` side is complete either way, so an IME is a source to add later
* **D7, key state tracked per keyboard model** (taken 2026-11-02). Which keys are down and the logical key each went down as, a down for a key already down, an up for one that is not, modifiers and locks synchronized with synthesized events: one tracker for XKB-style state, ported from fl_key_embedder_responder, which every Linux source shares (evdev or libinput, the script, later Wayland). Not one for every platform: Windows, macOS and the rest report modifiers and locks in ways a common model would need a hook for each, and their stock embedders are the references. Not one per source: the Linux sources would each carry the same 785 lines. A non-Linux platform brings its own; a common core is factored out at the third, if one shows. Its acceptance tests are fl_key_embedder_responder_test.cc's. Simpler than GTK's: the state comes from the keymap, known exactly, so GTK's inference of when lock state applies (`caps_lock_state_logic_inferrence`) is not ported. Where the stock embedder's events follow from that inference, the reference is checked for intent and the difference recorded
* **D8, a platform boundary at the embedder API's shape** (taken 2026-11-02). Events as data shaped as `embedder.h`'s `FlutterPointerEvent` and `FlutterKeyEvent` (view id, every device kind, timestamps on the frame clock), pushed from any thread; traits only where the platform acts (text input and IME, clipboard, cursor). A neutral core (`fl-input`: events, D7's tracker, the editing model) with no OS code, building and testing with no backend; backends by platform, not by library: `fl-input-script`, `fl-platform-kms` on drmkit, later Wayland, which owns its window and its input together. drmkit's C libraries are taken as they are, so exit criterion 8 reads "no C compiler"; fl-engine's Rust floor rises to drmkit's 1.94.1
* **D10, no X11, in any form** (taken 2026-11-02). Wayland replaces it: no X11 window or connection, no XWayland, no X11 tools in tests or references (Xvfb, xdotool), no X11 features of dependencies (`xkbcommon-x11`, `x11rb`). XKB keymaps and XCursor theme files are Wayland's own formats, xkbcommon and the cursor themes Wayland compositors and clients read, and are not X11
* **D9, drmkit owns KMS, display included** (taken 2026-11-02). One owner of the card: the seat hands out its descriptor and takes it back on a terminal switch, and the cursor plane is committed with the frame, so display, session, input and cursor share one stack. `fl-platform-kms` implements emblema's `ScanoutOutput`, the trait emblema keeps for a KMS stack other than its own drm-rs one; emblema renders, drmkit modesets, flips and reports each flip's sequence and time. Not emblema's `DrmScanoutTarget`, which opens the card itself, around the seat. M2's stage 6, KMS on the boards, is done this way as M4's stage 6, not first through drm-rs. To check there: drmkit's present and GBM paths take buffers rendered elsewhere, and the Pi 5 measurement (60 Hz, no blank missed) holds through drmkit

## 14. Risks

* **The packet converter's rules are subtle.** Its unit tests are the specification; ported case by case
* **UTF-16.** The framework counts text in UTF-16 code units, fl-text and Rust in UTF-8 bytes. Every offset crossing the channel is converted and checked, and a character outside the BMP is in every editing test
* **Keymap coverage.** If D3 falls back, non-US layouts wait. Each layout is data, not code
* **Event-reference drift.** A recorded reference ages with the pin; a pin bump re-records it (D5)
* **Board bring-up through drmkit.** emblema's own scanout was measured on a Pi 5; drmkit's path for emblema's buffers is not yet. Stage 6 measures it again, and the cursor plane with it

## 15. Reliability, Security and Performance

**Security: untrusted input**
* **Device input** (stage 6). Events from libinput are bounded on the way in: coordinates clamped to the view, touch slots and key codes checked against their tables, an unknown code dropped, not indexed with
* **Seat** (stage 6). Devices are held through the seat: a terminal switch revokes them and the display together, and keys never reach the console under a kiosk; a seat that cannot be had is reported, not worked around
* **Permissions** (stage 6). Without a seat (logind, seatd) devices cannot be opened; the backend names what is missing and the app runs without input
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
