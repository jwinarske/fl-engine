// The images program's scene (docs/m3-images-text.md §8), shared by main.dart
// and test/golden_test.dart so both engines draw the same tree.
import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Decodes every image the scene draws, by name, and logs each one's size.
/// Each source dart:ui offers: bytes from `flutter/assets`, an
/// `ImmutableBuffer` from an asset, a resize at decode, and raw pixels.
Future<Map<String, ui.Image>> decodeAll(AssetBundle bundle, void Function(String) log) async {
  final images = <String, ui.Image>{};
  Future<void> add(String name, Future<ui.Codec> codec) async {
    final c = await codec;
    final frame = await c.getNextFrame();
    images[name] = frame.image;
    log('$name: ${frame.image.width}x${frame.image.height} frames=${c.frameCount}');
  }

  for (final name in ['checker.png', 'photo.jpg', 'nine.png', 'atlas.png', 'quads.gif']) {
    final data = await bundle.load('assets/$name');
    await add(name, ui.instantiateImageCodec(data.buffer.asUint8List()));
  }
  // Through ImmutableBuffer.fromAsset, as image providers load.
  final buffer = await ui.ImmutableBuffer.fromAsset('assets/stripes.webp');
  await add('stripes.webp', ui.instantiateImageCodecFromBuffer(buffer));
  // Resized at decode: 64x64 to 16 wide, the height to match.
  final photo = await bundle.load('assets/photo.jpg');
  await add('photo.jpg@16', ui.instantiateImageCodec(photo.buffer.asUint8List(), targetWidth: 16));
  // Raw pixels: a 4x4 premultiplied RGBA ramp.
  final raw = Uint8List(64);
  for (var i = 0; i < 16; i++) {
    raw.setAll(i * 4, [i * 16, 255 - i * 16, 128, 255]);
  }
  final done = Completer<ui.Image>();
  ui.decodeImageFromPixels(raw, 4, 4, ui.PixelFormat.rgba8888, done.complete);
  images['raw'] = await done.future;
  log('raw: ${images['raw']!.width}x${images['raw']!.height}');
  return images;
}

/// The cells, 100x80 logical each, four across.
class Images extends StatelessWidget {
  const Images({super.key, required this.images});

  final Map<String, ui.Image> images;

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFFFFFFF),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            CustomPaint(size: const Size(400, 240), painter: _Cells(images)),
            // The framework's own path: AssetImage and the image cache.
            Padding(
              padding: const EdgeInsets.only(left: 5),
              child: Image.asset('assets/badge.png', width: 48, height: 48),
            ),
          ],
        ),
      ),
    );
  }
}

class _Cells extends CustomPainter {
  _Cells(this.images);

  final Map<String, ui.Image> images;

  static Offset _at(int i) => Offset((i % 4) * 100 + 5, (i ~/ 4) * 80 + 5);

  static Rect _cell(int i, double w, double h) => _at(i) & Size(w, h);

  ui.Image _img(String name) => images[name]!;

  @override
  void paint(Canvas canvas, Size size) {
    final plain = Paint();
    // 0: drawImage, at its own size.
    canvas.drawImage(_img('photo.jpg'), _at(0), plain);
    // 1: drawImageRect, the photo's middle magnified.
    canvas.drawImageRect(
      _img('photo.jpg'),
      const Rect.fromLTWH(16, 16, 32, 32),
      _cell(1, 70, 70),
      plain,
    );
    // 2: nine-slice, the 12x12 middle stretched.
    canvas.drawImageNine(
      _img('nine.png'),
      const Rect.fromLTRB(6, 6, 18, 18),
      _cell(2, 90, 70),
      plain,
    );
    // 3: drawAtlas, both sprites turned, tinted by modulate.
    final o = _at(3);
    canvas.drawAtlas(
      _img('atlas.png'),
      [
        RSTransform.fromComponents(
          rotation: 0.3,
          scale: 2,
          anchorX: 8,
          anchorY: 8,
          translateX: o.dx + 22,
          translateY: o.dy + 35,
        ),
        RSTransform.fromComponents(
          rotation: -0.4,
          scale: 2,
          anchorX: 8,
          anchorY: 8,
          translateX: o.dx + 66,
          translateY: o.dy + 35,
        ),
      ],
      const [Rect.fromLTWH(0, 0, 16, 16), Rect.fromLTWH(16, 0, 16, 16)],
      const [Color(0xFFE04020), Color(0xFF2060E0)],
      BlendMode.modulate,
      null,
      plain,
    );
    // 4: a rect filled by an ImageShader, the stripes doubled and repeated.
    final c4 = _cell(4, 90, 70);
    final m = Float64List.fromList([2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1, 0, c4.left, c4.top, 0, 1]);
    canvas.drawRect(
      c4,
      Paint()..shader = ImageShader(_img('stripes.webp'), TileMode.repeated, TileMode.repeated, m),
    );
    // 5 to 8: the checker magnified at each FilterQuality.
    for (final (i, q) in [
      (5, FilterQuality.none),
      (6, FilterQuality.low),
      (7, FilterQuality.medium),
      (8, FilterQuality.high),
    ]) {
      canvas.drawImageRect(
        _img('checker.png'),
        const Rect.fromLTWH(0, 0, 8, 8),
        _cell(i, 70, 70),
        Paint()..filterQuality = q,
      );
    }
    // 9: a GIF's first frame, a transparent quadrant.
    canvas.drawImageRect(
      _img('quads.gif'),
      const Rect.fromLTWH(0, 0, 16, 16),
      _cell(9, 64, 64),
      plain,
    );
    // 10: raw pixels.
    canvas.drawImageRect(_img('raw'), const Rect.fromLTWH(0, 0, 4, 4), _cell(10, 64, 64), plain);
    // 11: resized at decode, drawn at its decoded size.
    canvas.drawImage(_img('photo.jpg@16'), _at(11), plain);
  }

  @override
  bool shouldRepaint(_Cells old) => old.images != images;
}
