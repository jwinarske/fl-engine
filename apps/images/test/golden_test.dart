// The reference frame: the stock engine's flutter_tester renders Images at
// the size and DPR fl-engine's app lane uses (docs/m3-images-text.md §9).
// Decoding is real work, so it runs outside the fake async zone.
//
//     flutter test --update-goldens
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:images/images.dart';

void main() {
  testWidgets('images at 800x600, DPR 2', (tester) async {
    tester.view.physicalSize = const Size(800, 600);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    final images = (await tester.runAsync(() => decodeAll(rootBundle, (_) {})))!;
    await tester.pumpWidget(Images(images: images));
    await tester.runAsync(
      () =>
          precacheImage(const AssetImage('assets/badge.png'), tester.element(find.byType(Images))),
    );
    await tester.pump();
    await expectLater(find.byType(Images), matchesGoldenFile('goldens/images.png'));
  });
}
