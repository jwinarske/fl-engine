// A 4x4 grid of 100x75 cells on a 400x300 view, drawn by one CustomPainter:
// each cell is a group of Canvas and Path natives, named beside it. Over
// the bottom row's first two cells, the two clip layers the framework only
// pushes around a compositing child: ClipPath and ClipRRect over Opacity.
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

class Natives extends StatelessWidget {
  const Natives({super.key});

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFFFFFFF),
        child: Stack(
          children: [
            const Positioned.fill(child: CustomPaint(painter: _Cells())),
            // pushClipPath: a diamond.
            Positioned(
              left: 8,
              top: 228,
              width: 64,
              height: 64,
              child: ClipPath(
                clipper: _Diamond(),
                child: const Opacity(
                  opacity: 0.6,
                  child: ColoredBox(color: Color(0xFF00897B)),
                ),
              ),
            ),
            // pushClipRRect.
            Positioned(
              left: 88,
              top: 228,
              width: 64,
              height: 64,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(16),
                child: const Opacity(
                  opacity: 0.6,
                  child: ColoredBox(color: Color(0xFF3949AB)),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Diamond extends CustomClipper<Path> {
  @override
  Path getClip(Size size) => Path()
    ..moveTo(size.width / 2, 0)
    ..lineTo(size.width, size.height / 2)
    ..lineTo(size.width / 2, size.height)
    ..lineTo(0, size.height / 2)
    ..close();

  @override
  bool shouldReclip(_Diamond oldClipper) => false;
}

Paint _fill(int argb) => Paint()..color = Color(argb);

Paint _stroke(int argb, double width) => Paint()
  ..color = Color(argb)
  ..style = PaintingStyle.stroke
  ..strokeWidth = width;

class _Cells extends CustomPainter {
  const _Cells();

  static const _cell = Size(100, 75);

  @override
  void paint(Canvas canvas, Size size) {
    final cells = <void Function(Canvas)>[
      _oval,
      _drrect,
      _arcs,
      _curves,
      _pathArcs,
      _evenOdd,
      _relative,
      _addArcRRect,
      _pathOfPaths,
      _clipPathPaint,
      _clipOutColor,
      _layers,
      _transforms,
      _saveCount,
      _picture,
      _blendJoins,
    ];
    for (var i = 0; i < cells.length; i++) {
      canvas.save();
      canvas.translate((i % 4) * _cell.width, (i ~/ 4) * _cell.height);
      canvas.clipRect(Offset.zero & _cell);
      cells[i](canvas);
      canvas.restore();
    }
  }

  // drawOval
  static void _oval(Canvas c) =>
      c.drawOval(const Rect.fromLTRB(10, 10, 90, 65), _fill(0xFFE53935));

  // drawDRRect
  static void _drrect(Canvas c) => c.drawDRRect(
        RRect.fromRectAndRadius(const Rect.fromLTRB(10, 10, 90, 65), const Radius.circular(14)),
        RRect.fromRectAndRadius(const Rect.fromLTRB(24, 22, 76, 53), const Radius.circular(6)),
        _fill(0xFF43A047),
      );

  // drawArc, with and without the center
  static void _arcs(Canvas c) {
    const r = Rect.fromLTRB(8, 10, 58, 60);
    c.drawArc(r, 0, math.pi * 1.25, true, _fill(0xFF8E24AA));
    c.drawArc(r.shift(const Offset(36, 0)), -math.pi / 2, math.pi, false, _stroke(0xFF1E88E5, 5));
  }

  // quadraticBezierTo, cubicTo, conicTo; round joins and caps
  static void _curves(Canvas c) {
    final p = Path()
      ..moveTo(8, 60)
      ..quadraticBezierTo(25, 5, 45, 40)
      ..cubicTo(55, 70, 70, 0, 80, 30)
      ..conicTo(90, 60, 95, 15, 2);
    c.drawPath(
      p,
      _stroke(0xFFFB8C00, 5)
        ..strokeJoin = StrokeJoin.round
        ..strokeCap = StrokeCap.round,
    );
  }

  // arcTo, arcToPoint, relativeArcToPoint
  static void _pathArcs(Canvas c) {
    final p = Path()
      ..moveTo(10, 40)
      ..arcTo(const Rect.fromLTRB(10, 15, 50, 55), math.pi, math.pi / 2, false)
      ..arcToPoint(const Offset(70, 35), radius: const Radius.circular(20))
      ..relativeArcToPoint(const Offset(20, 20), radius: const Radius.circular(12), clockwise: false);
    c.drawPath(p, _stroke(0xFF6D4C41, 4));
  }

  // addOval, addRect, fillType evenOdd: a frame with an oval hole
  static void _evenOdd(Canvas c) {
    final p = Path()
      ..fillType = PathFillType.evenOdd
      ..addRect(const Rect.fromLTRB(10, 10, 90, 65))
      ..addOval(const Rect.fromLTRB(25, 20, 75, 55));
    // Read back (getFillType): an assert would be compiled out of release.
    if (p.fillType != PathFillType.evenOdd) {
      throw StateError('fillType did not round-trip');
    }
    c.drawPath(p, _fill(0xFF00ACC1));
  }

  // relativeMoveTo, relativeLineTo, relativeQuadraticBezierTo,
  // relativeCubicTo, relativeConicTo, addPolygon
  static void _relative(Canvas c) {
    final p = Path()
      ..relativeMoveTo(10, 60)
      ..relativeLineTo(15, -40)
      ..relativeQuadraticBezierTo(10, -15, 20, 0)
      ..relativeCubicTo(5, 20, 15, 20, 20, 0)
      ..relativeConicTo(10, -10, 20, 30, 0.7)
      ..addPolygon(const [Offset(10, 70), Offset(30, 66), Offset(20, 74)], true);
    c.drawPath(p, _stroke(0xFFD81B60, 3));
  }

  // addArc, addRRect
  static void _addArcRRect(Canvas c) {
    final p = Path()
      ..addRRect(RRect.fromLTRBAndCorners(10, 10, 55, 65,
          topLeft: const Radius.circular(18), bottomRight: const Radius.circular(8)))
      ..addArc(const Rect.fromLTRB(50, 15, 90, 55), 0, math.pi * 1.5);
    c.drawPath(p, _stroke(0xFF5E35B1, 4));
  }

  // addPath, addPathWithMatrix, shift, transform, Path.from (clone),
  // getBounds, reset
  static void _pathOfPaths(Canvas c) {
    final tri = Path()
      ..moveTo(0, 0)
      ..lineTo(20, 0)
      ..lineTo(10, 18)
      ..close();
    final m = Matrix4.rotationZ(math.pi / 6).storage;
    final p = Path()
      ..addPath(tri, const Offset(8, 8))
      ..addPath(tri, const Offset(40, 8), matrix4: m)
      ..addPath(tri.shift(const Offset(70, 8)), Offset.zero)
      ..addPath(tri.transform(Matrix4.diagonal3Values(1.5, 1.5, 1).storage), const Offset(10, 40));
    final copy = Path.from(p);
    c.drawPath(copy, _fill(0xFF7CB342));
    c.drawRect(p.getBounds(), _stroke(0xFF000000, 1));
    p.reset();
    c.drawPath(p, _fill(0xFFFF0000)); // empty: draws nothing
  }

  // clipPath, drawPaint
  static void _clipPathPaint(Canvas c) {
    c.clipPath(Path()..addOval(const Rect.fromLTRB(20, 8, 80, 68)));
    c.drawPaint(_fill(0xFFFDD835));
  }

  // clipRect with ClipOp.difference, drawColor
  static void _clipOutColor(Canvas c) {
    c.clipRect(const Rect.fromLTRB(30, 20, 70, 55), clipOp: ui.ClipOp.difference);
    c.drawColor(const Color(0xFF546E7A), BlendMode.srcOver);
  }

  // saveLayer with bounds and without: group opacity over overlapping draws
  static void _layers(Canvas c) {
    c.saveLayer(const Rect.fromLTRB(0, 0, 100, 75), Paint()..color = const Color(0x80000000));
    c.drawRect(const Rect.fromLTRB(10, 10, 60, 50), _fill(0xFFE53935));
    c.drawRect(const Rect.fromLTRB(35, 25, 85, 65), _fill(0xFF1E88E5));
    c.restore();
    c.saveLayer(null, Paint()..color = const Color(0xC0FFFFFF));
    c.drawCircle(const Offset(50, 38), 8, _fill(0xFF000000));
    c.restore();
  }

  // scale, rotate, skew
  static void _transforms(Canvas c) {
    c.translate(50, 38);
    c.rotate(math.pi / 10);
    c.skew(0.3, 0);
    c.scale(1.4, 0.8);
    c.drawRect(const Rect.fromLTRB(-20, -15, 20, 15), _fill(0xFF26A69A));
  }

  // getSaveCount, restoreToCount
  static void _saveCount(Canvas c) {
    final base = c.getSaveCount();
    c.save();
    c.translate(20, 0);
    c.save();
    c.translate(0, 20);
    c.restoreToCount(base);
    // Back where the cell began: the rect is at its own coordinates.
    c.drawRect(const Rect.fromLTRB(10, 10, 40, 40), _fill(0xFFEF6C00));
  }

  // drawPicture: a recorded picture drawn twice
  static void _picture(Canvas c) {
    final rec = ui.PictureRecorder();
    Canvas(rec).drawCircle(const Offset(15, 15), 12, _fill(0xFFAB47BC));
    final pic = rec.endRecording();
    c.drawPicture(pic);
    c.translate(50, 30);
    c.drawPicture(pic);
    pic.dispose();
  }

  // blend modulate (multiply needs advanced blending, which lavapipe lacks);
  // miter and bevel joins
  static void _blendJoins(Canvas c) {
    c.drawRect(const Rect.fromLTRB(5, 5, 45, 40), _fill(0xFFFFEB3B));
    c.drawRect(const Rect.fromLTRB(25, 20, 65, 55), _fill(0xFF29B6F6)..blendMode = BlendMode.modulate);
    final zig = Path()
      ..moveTo(55, 65)
      ..lineTo(70, 15)
      ..lineTo(85, 65);
    c.drawPath(zig, _stroke(0xFF000000, 6)..strokeJoin = StrokeJoin.miter);
    c.drawPath(zig.shift(const Offset(-50, 0)), _stroke(0xFF000000, 6)..strokeJoin = StrokeJoin.bevel);
  }

  @override
  bool shouldRepaint(_Cells oldDelegate) => false;
}
