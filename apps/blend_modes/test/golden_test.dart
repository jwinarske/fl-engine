// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders the cells at the size and DPR fl-engine's app lane uses.
//
//     flutter test --enable-impeller --update-goldens
import 'package:blend_modes/blend_modes.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('blend modes at 800x600, DPR 2', (tester) async {
    tester.view.physicalSize = const Size(1600, 1200);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const BlendModes());
    await expectLater(find.byType(BlendModes), matchesGoldenFile('goldens/blend_modes.png'));
  });
}
