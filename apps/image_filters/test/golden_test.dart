// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders the cells at the size and DPR fl-engine's app lane uses.
//
//     flutter test --enable-impeller --update-goldens
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image_filters/image_filters.dart';

void main() {
  testWidgets('image filters at 800x600, DPR 2', (tester) async {
    tester.view.physicalSize = const Size(1600, 1200);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const ImageFilters());
    await expectLater(find.byType(ImageFilters), matchesGoldenFile('goldens/image_filters.png'));
  });
}
