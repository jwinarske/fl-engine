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

Open: keys (stage 3). Mouse, scroll, pan-zoom and more than one finger from the script are stage 5's; the converter and the packet already carry them.

## Decisions taken 2026-11-16

D1 (devices and a script, no window), D4 (clipboard in the process), D5 and D6 (no system IME), each with what the stock Linux embedder does in its place (brief §13). D5 as taken: no recordings from the stock embedder; the engine's unit tests ported for each layer, and the same scripted input given to `flutter_tester` and to fl-dart end to end.
