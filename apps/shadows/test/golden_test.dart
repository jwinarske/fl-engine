// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders Shadows at the size and DPR fl-engine's app lane uses for it
// (docs/m3-images-text.md §9). DPR 3, not the other apps' 2: drawShadow
// reads the engine's view-0 DPR, which flutter_tester fixes at 3
// (tester_main.cc), so only at 3 do the shadow and the view agree.
//
//     flutter test --enable-impeller --update-goldens
import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shadows/shadows.dart';

void main() {
  testWidgets('shadows at 1200x900, DPR 3', (tester) async {
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    // flutter_test draws a physical model's outline, not its shadow, unless
    // told otherwise; fl-engine draws the shadow.
    debugDisableShadows = false;
    await tester.pumpWidget(const Shadows());
    await expectLater(find.byType(Shadows), matchesGoldenFile('goldens/shadows.png'));
    // Reset before the test ends: flutter_test checks it then.
    debugDisableShadows = true;
  });
}
