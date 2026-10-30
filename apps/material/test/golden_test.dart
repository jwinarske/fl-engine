// The reference frame: the stock engine's flutter_tester, with Impeller, renders Home in a
// MaterialApp at the size and DPR fl-engine's app lane uses
// (docs/m2-dart-ui.md §9).
//
//     flutter test --enable-impeller --update-goldens
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material/home.dart';

void main() {
  testWidgets('material at 800x600, DPR 2', (tester) async {
    tester.view.physicalSize = const Size(800, 600);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      const MaterialApp(debugShowCheckedModeBanner: false, restorationScopeId: 'material', home: Home()),
    );
    await expectLater(find.byType(MaterialApp), matchesGoldenFile('goldens/material.png'));
  });
}
