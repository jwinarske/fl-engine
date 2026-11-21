// The color filters program's scene, shared by main.dart and
// test/golden_test.dart so both engines draw the same tree.
import 'dart:async';
import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

const _red = Color(0xFFE53935);
const _yellow = Color(0xFFFDD835);
const _blue = Color(0xFF1E88E5);
const _tint = Color(0xFF8E24AA);

/// Roboto Bold from the Flutter release's material fonts, for the text
/// cells: flutter_tester draws any other family in its test font.
Future<void> loadFonts() async {
  final String root = Platform.environment['FLUTTER_ROOT'] ?? '';
  final File font = File('$root/bin/cache/artifacts/material_fonts/Roboto-Bold.ttf');
  await ui.loadFontFromList(font.readAsBytesSync(), fontFamily: 'Roboto');
}

/// A 4x4 image, a color a quadrant: red, green, blue, yellow, clockwise
/// from the top left.
Future<ui.Image> quadrants() {
  final px = Uint8List(64);
  for (var y = 0; y < 4; y++) {
    for (var x = 0; x < 4; x++) {
      final c = switch ((x < 2, y < 2)) {
        (true, true) => [255, 0, 0, 255],
        (false, true) => [0, 255, 0, 255],
        (true, false) => [255, 255, 0, 255],
        (false, false) => [0, 0, 255, 255],
      };
      px.setAll((y * 4 + x) * 4, c);
    }
  }
  final done = Completer<ui.Image>();
  ui.decodeImageFromPixels(px, 4, 4, ui.PixelFormat.rgba8888, done.complete);
  return done.future;
}

const _grayscale = ColorFilter.matrix(<double>[
  0.2126, 0.7152, 0.0722, 0, 0, //
  0.2126, 0.7152, 0.0722, 0, 0,
  0.2126, 0.7152, 0.0722, 0, 0,
  0, 0, 0, 1, 0,
]);

// Inverts, with the last column in dart:ui's 0-255 units.
const _invert = ColorFilter.matrix(<double>[
  -1, 0, 0, 0, 255, //
  0, -1, 0, 0, 255,
  0, 0, -1, 0, 255,
  0, 0, 0, 1, 0,
]);

// Half the alpha, and a constant added to green.
const _fade = ColorFilter.matrix(<double>[
  1, 0, 0, 0, 0, //
  0, 1, 0, 0, 64,
  0, 0, 1, 0, 0,
  0, 0, 0, 0.5, 0,
]);

/// Each filter, in the order the cells take them: the separable modes a
/// matrix states, the advanced ones it cannot, the matrices and the gammas.
const List<ColorFilter> filters = <ColorFilter>[
  ColorFilter.mode(_tint, BlendMode.srcIn),
  ColorFilter.mode(_tint, BlendMode.srcATop),
  ColorFilter.mode(Color(0x808E24AA), BlendMode.srcOver),
  ColorFilter.mode(_tint, BlendMode.modulate),
  ColorFilter.mode(_tint, BlendMode.multiply),
  ColorFilter.mode(_tint, BlendMode.screen),
  ColorFilter.mode(_tint, BlendMode.color),
  ColorFilter.mode(_tint, BlendMode.luminosity),
  ColorFilter.mode(_tint, BlendMode.colorBurn),
  ColorFilter.mode(_tint, BlendMode.overlay),
  _grayscale,
  _invert,
  _fade,
  ColorFilter.linearToSrgbGamma(),
  ColorFilter.srgbToLinearGamma(),
];

/// Cells 95x95 logical, eight across: each filter on a shape's paint, then
/// on an image, then on a run of text, then through saveLayer, then as a
/// `ColorFiltered` layer. A layer's filter has no bounds of its own, and
/// one that colors transparent black (srcOver) would fill the screen, as
/// it does in the stock engine, so each layer is clipped to its cell.
class ColorFilters extends StatelessWidget {
  const ColorFilters({super.key, required this.image});

  final ui.Image image;

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: Stack(
        children: <Widget>[
          CustomPaint(size: const Size(800, 600), painter: _Cells(image)),
          for (var j = 0; j < 4; j++)
            Positioned(
              left: (j % 8) * 100.0 + 2.5,
              top: 502.5,
              width: 95,
              height: 95,
              child: ClipRect(
                child: ColorFiltered(colorFilter: filters[j * 3], child: const _Sample()),
              ),
            ),
          for (var j = 0; j < 4; j++)
            Positioned(
              left: (j + 4) * 100.0 + 2.5,
              top: 502.5,
              width: 95,
              height: 95,
              child: ClipRect(
                child: ColorFiltered(colorFilter: filters[j * 3 + 1], child: const _Sample()),
              ),
            ),
        ],
      ),
    );
  }
}

/// What a layer filters: a gradient and a translucent circle.
class _Sample extends StatelessWidget {
  const _Sample();

  @override
  Widget build(BuildContext context) => CustomPaint(painter: _SamplePainter());
}

class _SamplePainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) => _shape(canvas, Offset.zero & size, null);

  @override
  bool shouldRepaint(_SamplePainter oldDelegate) => false;
}

/// Filters not affine in color: the advanced modes and the gamma curves.
/// Impeller filters a gradient's stop colors and interpolates the results
/// (`LinearGradientContents::ApplyColorFilter`), which is the filtered
/// gradient only for an affine filter; emblema filters each pixel, as Skia
/// does. Under these the square is solid, where the two agree.
const Set<int> _notAffine = <int>{6, 7, 8, 9, 13, 14};

/// A gradient square, or a solid one, and a translucent yellow circle over
/// it, each drawn with `filter` on its paint.
void _shape(Canvas canvas, Rect r, ColorFilter? filter, {bool gradient = true}) {
  final Paint square = Paint()..colorFilter = filter;
  if (gradient) {
    square.shader = ui.Gradient.linear(r.topLeft, r.bottomRight, <Color>[_red, _blue]);
  } else {
    square.color = _blue;
  }
  canvas.drawRect(r, square);
  canvas.drawCircle(
    r.center,
    r.width / 3,
    Paint()
      ..color = _yellow.withValues(alpha: 0.75)
      ..colorFilter = filter,
  );
}

class _Cells extends CustomPainter {
  _Cells(this.image);

  final ui.Image image;

  static Rect _cell(int i) => Offset((i % 8) * 100.0 + 2.5, (i ~/ 8) * 100.0 + 2.5) & const Size(95, 95);

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawColor(const Color(0xFFFFFFFF), BlendMode.src);
    // Rows 0 and 1: each filter on a shape's paints (15 cells), and one
    // unfiltered for reference.
    for (var i = 0; i < filters.length; i++) {
      _shape(canvas, _cell(i), filters[i], gradient: !_notAffine.contains(i));
    }
    _shape(canvas, _cell(15), null);

    // Rows 2 and 3: each filter on an image draw, scaled up unsmoothed.
    final src = Rect.fromLTWH(0, 0, image.width.toDouble(), image.height.toDouble());
    for (var i = 0; i < filters.length; i++) {
      canvas.drawImageRect(image, src, _cell(16 + i), Paint()..colorFilter = filters[i]);
    }
    canvas.drawImageRect(image, src, _cell(31), Paint());

    // Row 4: text with a filtered foreground paint, and saveLayer with a
    // filtered paint over an unfiltered shape.
    // Only filters that keep transparent black so: Impeller filters a run's
    // glyph quads whole, so one that colors transparent black (multiply)
    // fills each glyph's box.
    const textFilters = <int>[0, 3, 10, 11];
    for (var i = 0; i < 4; i++) {
      final r = _cell(32 + i);
      final paragraph =
          (ui.ParagraphBuilder(ui.ParagraphStyle(fontFamily: 'Roboto', fontSize: 40))
                ..pushStyle(
                  ui.TextStyle(
                    fontFamily: 'Roboto',
                    foreground: Paint()
                      ..color = _red
                      ..colorFilter = filters[textFilters[i]],
                  ),
                )
                ..addText('Ab'))
              .build()
            ..layout(const ui.ParagraphConstraints(width: 95));
      canvas.drawParagraph(paragraph, r.topLeft);
    }
    for (var i = 0; i < 4; i++) {
      final r = _cell(36 + i);
      // The bounds do not clip the filter, so the cell does.
      canvas.save();
      canvas.clipRect(r);
      canvas.saveLayer(r, Paint()..colorFilter = filters[i * 4 + 2]);
      _shape(canvas, r, null);
      canvas.restore();
      canvas.restore();
    }
  }

  @override
  bool shouldRepaint(_Cells oldDelegate) => false;
}
