// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders the sheet at the size and DPR fl-engine's app lane uses.
//
//     flutter test --enable-impeller --update-goldens
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:text/text.dart';

void main() {
  testWidgets('text at 1600x1200, DPR 2', (tester) async {
    await tester.runAsync(loadFonts);
    tester.view.physicalSize = const Size(1600, 1200);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const TextSheet());
    await expectLater(find.byType(TextSheet), matchesGoldenFile('goldens/text.png'));
  });
}
