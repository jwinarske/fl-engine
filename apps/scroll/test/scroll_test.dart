// The reference: the stock framework under flutter_tester, given the
// scrolls test/scroll.txt gives fl-dart, logs where each leaves the list
// (docs/m4-input.md exit criterion 4).
//
//     flutter test --update-goldens
//
// The events are what the engine's packet converter makes of the
// embedder's, sent to the framework's binding with the script's times
// between them: a fling's speed is read from its events' times.
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:scroll/list.dart';

void main() {
  testWidgets('scroll', (WidgetTester tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    final List<String> lines = <String>[];
    await tester.pumpWidget(ScrollLog(log: lines.add));

    // The script's time: its waits, added up. The framework's own clock
    // goes on by as much: a pointer that has not moved for a while is
    // taken to have stopped, by that clock and not by its events' times.
    Duration at = Duration.zero;
    Future<void> wait(int ms) async {
      at += Duration(milliseconds: ms);
      await tester.pump(Duration(milliseconds: ms));
    }

    Future<void> send(PointerEvent e) => tester.sendEventToBinding(e);

    // The wheel, three notches down, at the view's middle.
    final TestPointer mouse = TestPointer(1, PointerDeviceKind.mouse, 0);
    await send(mouse.addPointer(location: const Offset(200, 150), timeStamp: at));
    await send(mouse.scroll(const Offset(0, 159), timeStamp: at));
    await tester.pumpAndSettle();

    // A drag, and a rest before the finger lifts.
    final TestPointer finger = TestPointer(2, PointerDeviceKind.touch, 2 << 28);
    await send(finger.addPointer(location: const Offset(200, 200), timeStamp: at));
    await send(finger.down(const Offset(200, 200), timeStamp: at));
    for (final double y in <double>[180, 160, 140, 120, 100]) {
      await wait(16);
      await send(finger.move(Offset(200, y), timeStamp: at));
    }
    await wait(200);
    await send(finger.up(timeStamp: at));
    await send(finger.removePointer(location: const Offset(200, 100), timeStamp: at));
    await tester.pumpAndSettle();

    // A fling.
    final double start = 700 / 3;
    await send(finger.addPointer(location: Offset(200, start), timeStamp: at));
    await send(finger.down(Offset(200, start), timeStamp: at));
    for (int i = 1; i <= 5; i++) {
      await wait(8);
      await send(finger.move(Offset(200, start - 12.0 * i), timeStamp: at));
    }
    await send(finger.up(timeStamp: at));
    await send(finger.removePointer(location: Offset(200, start - 60), timeStamp: at));
    await tester.pumpAndSettle();
    await wait(100);

    // A touchpad's pan: half a notch, half a notch, a notch, the content
    // following the fingers.
    final TestPointer pad = TestPointer(3, PointerDeviceKind.trackpad, 1);
    await send(pad.addPointer(location: const Offset(200, 150), timeStamp: at));
    await send(pad.panZoomStart(const Offset(200, 150), timeStamp: at));
    await send(pad.panZoomUpdate(const Offset(200, 150), pan: const Offset(0, -26.5), timeStamp: at));
    await wait(16);
    await send(pad.panZoomUpdate(const Offset(200, 150), pan: const Offset(0, -53), timeStamp: at));
    await wait(16);
    await send(pad.panZoomUpdate(const Offset(200, 150), pan: const Offset(0, -106), timeStamp: at));
    await wait(200);
    await send(pad.panZoomEnd(timeStamp: at));
    await tester.pumpAndSettle();

    // The wheel, a notch back.
    await send(mouse.scroll(const Offset(0, -53), timeStamp: at));
    await tester.pumpAndSettle();

    final String out = '${lines.where((String l) => l.startsWith('settled ')).join('\n')}\n';
    final File expected = File('test/scroll_settled.txt');
    if (autoUpdateGoldenFiles) {
      expected.writeAsStringSync(out);
    } else {
      expect(out, expected.readAsStringSync());
    }
    debugDefaultTargetPlatformOverride = null;
  });
}
