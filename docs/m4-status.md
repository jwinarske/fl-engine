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

## Decisions taken 2026-11-16

D1 (devices and a script, no window), D4 (clipboard in the process), D5 and D6 (no system IME), each with what the stock Linux embedder does in its place (brief §13). D5 as taken: no recordings from the stock embedder; the engine's unit tests ported for each layer, and the same scripted input given to `flutter_tester` and to fl-dart end to end.
