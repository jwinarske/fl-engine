// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders the scene with the producer's first pattern as an image where
// fl-engine shows the texture, at the size and DPR fl-engine's app lane
// uses.
//
//     flutter test --enable-impeller --update-goldens
import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:textures/textures.dart';

void main() {
  testWidgets('textures at 800x600, DPR 2', (tester) async {
    tester.view.physicalSize = const Size(1600, 1200);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    final ui.Image image = (await tester.runAsync(() async {
      final ui.ImmutableBuffer buffer = await ui.ImmutableBuffer.fromUint8List(pattern(0));
      final ui.ImageDescriptor descriptor = ui.ImageDescriptor.raw(
        buffer,
        width: side,
        height: side,
        pixelFormat: ui.PixelFormat.rgba8888,
      );
      final ui.Codec codec = await descriptor.instantiateCodec();
      return (await codec.getNextFrame()).image;
    }))!;
    await tester.pumpWidget(
      Textures(
        view: (FilterQuality quality) => RawImage(image: image, fit: BoxFit.fill, filterQuality: quality),
      ),
    );
    await expectLater(find.byType(Textures), matchesGoldenFile('goldens/textures.png'));
  });
}
