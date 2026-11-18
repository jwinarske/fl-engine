# Milestone 4 exit report

State of the §11 exit criteria and the §15 list of the brief (docs/m4-input.md), with the evidence for each. Evidence is a CI lane, a test, or a recorded finding. Pin: Dart 3.13.7, Flutter 3.47.8, drmkit 0.1.6, emblema 0.3.3. References: the stock engine's `flutter_tester` given the same input, and the engine's own unit tests, ported (D5).

Every exit criterion is met, and every §15 limit has a test that crosses it. On the Pi 5 the devices were made through `/dev/uinput`, a real mouse aside; the VisionFive 2's were a real keyboard and mouse. No real touchpad or touch screen has been tried.

## §11 exit criteria

| # | Criterion | State | Evidence |
|---|---|---|---|
| 1 | `counter`, unmodified, counts three scripted taps and matches its golden | done | ci.yml `Flutter apps`: `counter_counts_three_scripted_taps`, the last of 74 frames against `counter_tapped.png`, 1.065% differ, 0.183% over 32 |
| 2 | `text_field` takes text, a composed character, a dragged selection, copy and paste, and matches its golden after each | done | `text_field_edits_match_the_stock_framework`: frames after the typing, the selection and the paste against their goldens, and every editing value the one `flutter_tester` logs |
| 3 | `keys` and `pointer` match `flutter_tester` for the same input; the synthesized events match the engine's unit tests, ported | done | `keys_match_the_stock_framework`, `pointers_match_the_stock_framework`. Ported, case for case under their names: fl-ui `pointer`, `PointerDataPacketConverter`'s 12; fl-input `xkb::tracker_tests`, 16 of `fl_key_embedder_responder_test.cc`'s 17; `pointer_tests`, the pointer, scrolling and touch managers' |
| 4 | `scroll` scrolls by wheel, drag and fling to the stock engine's offsets within a pixel | done | `scroll_settles_where_the_stock_framework_does`: five rests (wheel, drag, fling, touchpad pan, wheel back), each within a logical pixel of `apps/scroll/test/scroll_settled.txt` |
| 5 | Every message on `flutter/textinput`, `flutter/keyboard`, `flutter/platform`'s clipboard and `flutter/mousecursor` is answered | done | `messages_are_answered_every_way`; `every_system_cursor_is_asked_for_by_its_theme_name`; fl-ui `malformed_calls_are_errors`, `a_kind_is_shown_by_its_theme_name_and_every_call_is_answered`, `the_keyboard_state_is_a_standard_codec_map`; the channel fuzz target answers every call or names it no call; soak `persistent_handles=0` |
| 6 | The input thread never touches Dart: events are `Send`, the converter and editing state are not, each with a compile-fail case | done, one difference | fl-ui `tests/compile_fail/converter_crosses_threads`; fl-input `events_cross_threads`. The editing state is `Send` and behind fl-ui's mutex (below) |
| 7 | Leak, sanitizer and fuzz lanes green with the M4 programs; every §15 limit crossed by a test | done | soak.yml `input_cycles_stay_flat` (counter, pointer, scroll, keys, text_field); sanitize.yml `apps_under_a_sanitizer` with `jit,kms`, ASan and TSan; fuzz: fl-input and fl-input-script `--test fuzz`, fl-ui's channel fuzz, short in ci.yml, long under ASan. Limits: §15 below |
| 8 | No C compiler: Cargo builds no C or C++, the platform's libraries are linked, not built; the no-C++ lane covers the build. The core crates link none of them | done | ci.yml `no C++ toolchain` (`ci/no-cxx.sh`): `fl-dart` built with `--features kms` with no C++ compiler present and no crate that builds C in its tree, linking the system's libseat, libinput, libudev and libxkbcommon. ci.yml `Input boundary`: fl-ui and fl-shell depend on no backend, fl-input on nothing |

Criterion 6: the text input handler sits behind fl-ui's mutex with the channel that changes it, so it is the UI thread's by where it is called from, not by its type; its input method must be `Send` to be put there (m4-status.md, stage 7).

Criterion 8: gcc is in the lane as the linker driver rustc calls. `kms` is a feature that is asked for, so the default `fl-dart` links none of the four libraries.

## §15 list

| Item | State | Verified by |
|---|---|---|
| Device input | done | fl-platform-kms `a_mouse_moves_from_the_middle_and_stays_in_the_view`; positions that are not finite to zero, a key code past the keymap's range dropped, 64 fingers remembered; fl-input `fingers_past_the_last_id_are_bounded` |
| Seat | done | devices and the card opened through libseat alone; `keys_reach_the_framework_and_the_terminal_keys_reach_the_seat`, `what_is_down_comes_up_when_the_devices_go`. Pi 5: Ctrl+Alt+F2 and back, the session paused and resumed, the function key not told to the framework |
| Permissions | done, one difference | no seat is an error that says what is missing (`SeatError`); `--kms` then does not run, the display being the seat's too. The board kit counts it a SKIP |
| Channel payloads | done | fl-ui `malformed_calls_are_errors`, `nesting_is_bounded`, `what_is_not_json_is_none`; fl-input `offsets_outside_the_text_are_refused`, the model's `set_selection_outside_string`, `set_composing_range_outside_string`; fuzz |
| Every message answered | done | fl-shell `a_response_runs_its_message_s_continuation_once`, `closing_answers_every_message_with_none`, `a_key_is_handled_if_either_answer_says_so`; soak: no persistent handle left |
| Bounded queue | done, one difference | fl-shell `past_its_limit_the_input_queue_drops_motion_and_nothing_else` (4,096), `keys_are_queued_in_their_place_and_never_dropped`. Moves are not merged per device: the engine's default dispatcher does not, and resampling is the framework's |
| Key state | done | fl-input `xkb::tracker_tests`; `keys_match_the_stock_framework`; fl-platform-kms `a_down_for_a_key_down_is_a_repeat_and_changes_nothing`, `what_is_down_comes_up_when_the_devices_go` |
| Hotplug | done | Pi 5: a keyboard unplugged with a key down, its up; a mouse with a button down, the pointer's up; a touch screen with a finger down, a cancel. `the_pointer_leaves_with_the_mouse_that_moved_it`, `a_finger_is_a_pointer_from_its_down_to_its_up` |
| Latency | done | soak: the 90th percentile of the frames input asked for held to two frames (33,334 us); counter 16,989 us, 17,056 at most |
| Typing | done | soak: text_field, 10,000 characters, a frame each, built in 891 us on average against 4,000 |
| Boards | open | Pi 5: 20 frames asked for by a click in each of three runs, the longest 16.7, 16.7 and 16.2 ms, none dropped. VisionFive 2: `counter`'s frame 5.3 ms of the raster thread's 16.7 (11.2 with emblema 0.3.1, 15.5 before), each begun a blank after the last when tapped by a script. No latency figure but its first frame's |

The other limits crossed (exit criterion 7): fl-input-script `a_script_is_bounded_in_size_and_its_errors_in_length` (a megabyte; waits, positions, notches and fingers); fl-ui JSON's depth of 32; fl-input's 128 touch ids.

## Boards

* **Pi 5**, 1280x1440 at 60 Hz on HDMI, V3D: the display (300 frames, no blank missed), keys, mouse, wheel, touch, a touchpad's pinch, the cursor plane, a terminal switch away and back, hotplug, the board kit's `run.sh --input`, latency. Each in m4-status.md under stage 6
* **VisionFive 2**, 1280x720 at 60 Hz on HDMI (`--mode`; the monitor's own is 3840x2160 at 30), PowerVR BXE-4-32, buffers from GBM: the kit's nine checks and the display (300 frames, no blank missed, with the CPU's governor at `performance`); a real keyboard and mouse, the wheel, all 35 cursors, a capture, a terminal switch away and back; a keyboard and a mouse unplugged and plugged in again, both working after, a held key coming up as its keyboard went. Not a touch screen

## Open, carried forward

* On the VisionFive 2: the cursor is sometimes noise when it first appears, until the mouse is moved further, its buffer right all the while; perhaps the board's display driver (#29, m4-status.md). The first hover and the first click each waited most of a second for a pipeline built at its first use, the button seeming dead until then; with emblema 0.3.3 the pipelines are kept between runs and that is a board's first run alone: by hand the first hover and click answer (m4-status.md, its frame). A touch screen, a latency figure; a key held as its keyboard is unplugged comes up (`apps/keys`' log); a mouse's button the same way has not been seen. A frame's draw was 9 ms there whatever is drawn, 12 to 14 before emblema 0.3.1 kept two buffers it made and freed each frame; with 0.3.2 the GPU's 5.5 to 6 of it is the kernel's to wait for and the thread's part is 3.5 (m4-status.md, its frame). What the display shows was read back on that path (`--capture`) and is the frame drawn offscreen, to one level in 255. On the panel, looked at on 2026-11-18: text_churn for 30 seconds with no tearing, `counter` clicked to 94 with a real mouse, the frame whole after each of two terminal switches; the cursor's noise at first as before
* The cursor's shape was looked at on the Pi 5's screen on 2026-11-17, a real mouse taken across `apps/cursors`: it changes in each cell. Each shape against its name is not recorded one by one
* A real keyboard, touchpad and touch screen; a rotation and a canceled pinch from a device
* The devices' thread under TSan: CI's runners have no devices, so the sanitizer lanes run the script's thread. The device source's state is driven in fl-platform-kms's tests with drmkit-input's events
* From the device to the screen: latency is measured from the event's arrival to the end of `_drawFrame`
* A display plugged in or unplugged while an app runs: the app keeps the connector it started on. Not in the brief
* A system input method (IBus, Fcitx) and a system clipboard: later sources (D4, D6)
* `flutter/platform` methods other than the clipboard's are dropped unanswered, as before M4

Detail: docs/m4-status.md.
