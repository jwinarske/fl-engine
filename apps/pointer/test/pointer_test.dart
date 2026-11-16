// The reference lines: the stock framework under flutter_tester, sent the
// pointers test/pointers.txt sends fl-dart, logs what it sees of them
// (docs/m4-input.md §9, D5).
//
//     flutter test --update-goldens
//
// flutter_tester gives its events to the framework's binding, above the
// engine and its packet converter. So the test sends what the converter
// makes of the embedder's events, its synthesized ones among them: an add
// before a pointer's first event, a hover to where a pointer leaves or a
// gesture begins. What is held is that fl-engine's managers, converter and
// packet come to these events.
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pointer/pointers.dart';

void main() {
  testWidgets('pointers', (WidgetTester tester) async {
    tester.view.physicalSize = const Size(400, 300);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final List<String> lines = <String>[];
    await tester.pumpWidget(PointerLog(log: lines.add));
    logRoute(lines.add);
    Future<void> send(PointerEvent e) => tester.sendEventToBinding(e);

    // A mouse hovers in, drags, and a second button goes down and up
    // within the drag. Its first hover is where it was added: no event.
    final TestPointer mouse = TestPointer(1, PointerDeviceKind.mouse, 0);
    await send(mouse.addPointer(location: const Offset(10, 20)));
    await send(mouse.hover(const Offset(30, 40)));
    await send(mouse.down(const Offset(30, 40)));
    await send(mouse.move(const Offset(50, 60)));
    await send(mouse.move(const Offset(50, 60), buttons: kPrimaryMouseButton | kSecondaryMouseButton));
    await send(mouse.move(const Offset(50, 60), buttons: kPrimaryMouseButton));
    await send(mouse.move(const Offset(70, 80)));
    await send(mouse.up());

    // Its wheel: a notch is 53 logical pixels.
    await send(mouse.scroll(const Offset(0, 53)));
    await send(mouse.scroll(const Offset(-26.5, 0)));

    // It leaves, hovering to where it went, and is back.
    await send(mouse.hover(const Offset(0, 80)));
    await send(mouse.removePointer(location: const Offset(0, 80)));
    await send(mouse.addPointer(location: const Offset(5, 5)));

    // Two fingers.
    final TestPointer first = TestPointer(2, PointerDeviceKind.touch, 2 << 28);
    final TestPointer second = TestPointer(3, PointerDeviceKind.touch, 2 << 28 | 1);
    await send(first.addPointer(location: const Offset(100, 100)));
    await send(first.down(const Offset(100, 100)));
    await send(second.addPointer(location: const Offset(200, 100)));
    await send(second.down(const Offset(200, 100)));
    await send(first.move(const Offset(110, 120)));
    await send(second.move(const Offset(210, 120)));
    await send(first.up());
    await send(first.removePointer(location: const Offset(110, 120)));
    await send(second.up());
    await send(second.removePointer(location: const Offset(210, 120)));

    // A touchpad pans where the fingers' scroll was, then pinches and
    // turns where the mouse last was, which it hovers to.
    final TestPointer pad = TestPointer(4, PointerDeviceKind.trackpad, 1);
    await send(pad.addPointer(location: const Offset(150, 150)));
    await send(pad.panZoomStart(const Offset(150, 150)));
    await send(pad.panZoomUpdate(const Offset(150, 150), pan: const Offset(0, 53)));
    await send(pad.panZoomUpdate(const Offset(150, 150), pan: const Offset(-53, 106)));
    await send(pad.panZoomEnd());
    await send(pad.hover(const Offset(5, 5)));
    await send(pad.panZoomStart(const Offset(5, 5)));
    await send(pad.panZoomUpdate(const Offset(5, 5), scale: 1.5));
    await send(pad.panZoomUpdate(const Offset(5, 5), scale: 1.5, rotation: 0.5));
    await send(pad.panZoomEnd());

    final String out = '${lines.join('\n')}\n';
    final File expected = File('test/pointers_events.txt');
    if (autoUpdateGoldenFiles) {
      expected.writeAsStringSync(out);
    } else {
      expect(out, expected.readAsStringSync());
    }
  });
}
