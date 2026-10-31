// The reference: the stock engine's flutter_tester, with Impeller, decodes
// the same animations (docs/m3-images-text.md §9). Its report is
// test/report.txt; its sheet, the golden. Decoding is real work, so it
// runs outside the fake async zone.
//
//     flutter test --enable-impeller --update-goldens
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gif/gif.dart';

void main() {
  testWidgets('report', (tester) async {
    final lines = (await tester.runAsync(() => report(rootBundle)))!;
    final out = '${lines.join('\n')}\n';
    final expected = File('test/report.txt');
    if (autoUpdateGoldenFiles) {
      expected.writeAsStringSync(out);
    } else {
      expect(out, expected.readAsStringSync());
    }
  });

  testWidgets('sheet at 400x300, DPR 1', (tester) async {
    tester.view.physicalSize = const Size(400, 300);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final frames = (await tester.runAsync(() => decodeFrames(rootBundle)))!;
    // The first frame the steps animation shows in: its first, red. The
    // framework gives it at a frame callback, so frames are pumped while
    // the decode is waited on outside the fake zone; no fake time passes,
    // so it stays the first. fl-engine's lane picks the same frame.
    await tester.pumpWidget(Sheet(frames: frames));
    var loaded = false;
    final stream = steps.resolve(ImageConfiguration.empty);
    final listener = ImageStreamListener((_, _) => loaded = true);
    stream.addListener(listener);
    while (!loaded) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 10)));
      await tester.pump();
    }
    await expectLater(find.byType(Sheet), matchesGoldenFile('goldens/gif.png'));
    // Without a listener the animation's timer for its next frame goes.
    stream.removeListener(listener);
    await tester.pumpWidget(const SizedBox());
  });
}
