// The reference frame: the stock engine's flutter_tester renders Shapes at
// the size and DPR fl-engine's app lane uses (docs/m2-dart-ui.md §9).
//
//     flutter test --update-goldens
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shapes/shapes.dart';

void main() {
  testWidgets('shapes at 800x600, DPR 2', (tester) async {
    tester.view.physicalSize = const Size(800, 600);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const Shapes());
    await expectLater(find.byType(Shapes), matchesGoldenFile('goldens/shapes.png'));
  });
}
