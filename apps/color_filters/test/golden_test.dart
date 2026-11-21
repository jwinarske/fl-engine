// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders the cells at the size and DPR fl-engine's app lane uses.
//
//     flutter test --enable-impeller --update-goldens
import 'package:color_filters/color_filters.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('color filters at 800x600, DPR 2', (tester) async {
    await tester.runAsync(loadFonts);
    tester.view.physicalSize = const Size(1600, 1200);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    final image = (await tester.runAsync(quadrants))!;
    await tester.pumpWidget(ColorFilters(image: image));
    await expectLater(find.byType(ColorFilters), matchesGoldenFile('goldens/color_filters.png'));
  });
}
