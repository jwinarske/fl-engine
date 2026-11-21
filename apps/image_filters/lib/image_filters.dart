// The image filters program's scene, shared by main.dart and
// test/golden_test.dart so both engines draw the same tree.
import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

const _red = Color(0xFFE53935);
const _yellow = Color(0xFFFDD835);
const _blue = Color(0xFF1E88E5);

const ColorFilter _grayscale = ColorFilter.matrix(<double>[
  0.2126, 0.7152, 0.0722, 0, 0, //
  0.2126, 0.7152, 0.0722, 0, 0,
  0.2126, 0.7152, 0.0722, 0, 0,
  0, 0, 0, 1, 0,
]);

/// Each filter, in the order the cells take them.
final List<ui.ImageFilter> filters = <ui.ImageFilter>[
  ui.ImageFilter.blur(sigmaX: 2, sigmaY: 2),
  ui.ImageFilter.blur(sigmaX: 6, sigmaY: 6),
  ui.ImageFilter.blur(sigmaX: 8, sigmaY: 1),
  ui.ImageFilter.dilate(radiusX: 3, radiusY: 3),
  ui.ImageFilter.erode(radiusX: 3, radiusY: 3),
  _grayscale,
  ui.ImageFilter.compose(outer: _grayscale, inner: ui.ImageFilter.blur(sigmaX: 4, sigmaY: 4)),
  ui.ImageFilter.compose(
    outer: ui.ImageFilter.blur(sigmaX: 2, sigmaY: 2),
    inner: ui.ImageFilter.dilate(radiusX: 4, radiusY: 4),
  ),
];

/// Cells 95x95 logical, eight across. Row 0: each filter on the paints of a
/// shape. Row 1: each through saveLayer. Row 2: each as an `ImageFiltered`
/// layer. Rows 3 and 4: each as a `BackdropFilter` over stripes, clipped to
/// its cell, with a translucent panel over it, as a frosted card is drawn.
class ImageFilters extends StatelessWidget {
  const ImageFilters({super.key});

  static Offset _at(int i) => Offset((i % 8) * 100.0 + 2.5, (i ~/ 8) * 100.0 + 2.5);

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: Stack(
        children: <Widget>[
          CustomPaint(size: const Size(800, 600), painter: _Cells()),
          for (var j = 0; j < filters.length; j++)
            Positioned.fromRect(
              rect: _at(16 + j) & const Size(95, 95),
              child: ImageFiltered(
                imageFilter: filters[j],
                child: CustomPaint(painter: _SamplePainter()),
              ),
            ),
          for (var j = 0; j < filters.length; j++)
            Positioned.fromRect(
              rect: _at(24 + j) & const Size(95, 95),
              child: ClipRect(
                child: BackdropFilter(
                  filter: filters[j],
                  child: const ColoredBox(color: Color(0x40FFFFFF)),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _SamplePainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) => _shape(canvas, Offset.zero & size, null);

  @override
  bool shouldRepaint(_SamplePainter oldDelegate) => false;
}

/// A red square inset in the cell, a yellow circle over it and a thin blue
/// line across, each drawn with `filter` on its paint.
void _shape(Canvas canvas, Rect r, ui.ImageFilter? filter) {
  Paint paint(Color c) => Paint()
    ..color = c
    ..imageFilter = filter;
  canvas.drawRect(r.deflate(20), paint(_red));
  canvas.drawCircle(r.center, 22, paint(_yellow));
  canvas.drawRect(Rect.fromLTWH(r.left + 10, r.center.dy - 2, r.width - 20, 4), paint(_blue));
}

/// Stripes for the backdrops to filter: blue and yellow, 8 logical wide,
/// across rows 3 and 4.
void _stripes(Canvas canvas) {
  final Rect area = const Offset(0, 300) & const Size(800, 200);
  canvas.save();
  canvas.clipRect(area);
  canvas.drawRect(area, Paint()..color = _yellow);
  for (var x = 0.0; x < 800; x += 16) {
    canvas.drawRect(Rect.fromLTWH(x, 300, 8, 200), Paint()..color = _blue);
  }
  canvas.drawCircle(const Offset(400, 400), 60, Paint()..color = _red);
  canvas.restore();
}

class _Cells extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawColor(const Color(0xFFFFFFFF), BlendMode.src);
    for (var i = 0; i < filters.length; i++) {
      final Rect r = ImageFilters._at(i) & const Size(95, 95);
      canvas.save();
      canvas.clipRect(r);
      _shape(canvas, r, filters[i]);
      canvas.restore();
    }
    for (var i = 0; i < filters.length; i++) {
      final Rect r = ImageFilters._at(8 + i) & const Size(95, 95);
      canvas.save();
      canvas.clipRect(r);
      canvas.saveLayer(r, Paint()..imageFilter = filters[i]);
      _shape(canvas, r, null);
      canvas.restore();
      canvas.restore();
    }
    _stripes(canvas);
  }

  @override
  bool shouldRepaint(_Cells oldDelegate) => false;
}
