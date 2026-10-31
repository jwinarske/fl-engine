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
    // The first frame: the steps animation not yet loaded, as in any
    // engine, the framework giving an animation's first frame at a frame
    // callback.
    await tester.pumpWidget(Sheet(frames: frames));
    await expectLater(find.byType(Sheet), matchesGoldenFile('goldens/gif.png'));
    await tester.pumpWidget(const SizedBox());
  });
}
