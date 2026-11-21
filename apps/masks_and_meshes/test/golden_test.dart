// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders the cells at the size and DPR fl-engine's app lane uses.
//
//     flutter test --enable-impeller --update-goldens
import 'package:masks_and_meshes/masks_and_meshes.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('masks and meshes at 800x600, DPR 2', (tester) async {
    tester.view.physicalSize = const Size(1600, 1200);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    final image = (await tester.runAsync(quadrants))!;
    await tester.pumpWidget(MasksAndMeshes(image: image));
    await expectLater(find.byType(MasksAndMeshes), matchesGoldenFile('goldens/masks_and_meshes.png'));
  });
}
