// One cell per drawing path, on a 400x300 logical view: what each widget
// asks of dart:ui is in the comment beside it.
import 'dart:math' as math;

import 'package:flutter/widgets.dart';

class Shapes extends StatelessWidget {
  const Shapes({super.key});

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFFFFFFF),
        child: Stack(
          children: [
            // drawRect
            _cell(0, 0, const ColoredBox(color: Color(0xFFE53935))),
            // drawRRect
            _cell(
              1,
              0,
              DecoratedBox(
                decoration: BoxDecoration(
                  color: const Color(0xFF43A047),
                  borderRadius: BorderRadius.circular(16),
                ),
              ),
            ),
            // drawPath: a triangle
            _cell(2, 0, const CustomPaint(painter: _Triangle())),
            // drawCircle
            _cell(
              0,
              1,
              const DecoratedBox(
                decoration: BoxDecoration(
                  color: Color(0xFF8E24AA),
                  shape: BoxShape.circle,
                ),
              ),
            ),
            // clipRRect on the canvas
            _cell(
              1,
              1,
              ClipRRect(
                borderRadius: BorderRadius.circular(24),
                child: const ColoredBox(color: Color(0xFF1E88E5)),
              ),
            ),
            // drawRect, stroked
            _cell(
              2,
              1,
              DecoratedBox(
                decoration: BoxDecoration(
                  border: Border.all(color: const Color(0xFF000000), width: 4),
                ),
              ),
            ),
            // canvas transform: a rotated rect
            _cell(
              0,
              2,
              Transform.rotate(
                angle: math.pi / 8,
                child: const ColoredBox(color: Color(0xFFFB8C00)),
              ),
            ),
            // pushClipRect over pushOpacity: half-transparent black
            _cell(
              1,
              2,
              const ClipRect(
                child: Opacity(
                  opacity: 0.5,
                  child: ColoredBox(color: Color(0xFF000000)),
                ),
              ),
            ),
            // drawLine, round caps
            _cell(2, 2, const CustomPaint(painter: _Line())),
          ],
        ),
      ),
    );
  }

  /// Column `x`, row `y` of a 3x3 grid of 112x72 cells, 16 apart.
  static Widget _cell(int x, int y, Widget child) {
    return Positioned(
      left: 16.0 + x * 128,
      top: 16.0 + y * 96,
      width: 112,
      height: 72,
      child: child,
    );
  }
}

class _Triangle extends CustomPainter {
  const _Triangle();

  @override
  void paint(Canvas canvas, Size size) {
    final path = Path()
      ..moveTo(size.width / 2, 0)
      ..lineTo(size.width, size.height)
      ..lineTo(0, size.height)
      ..close();
    canvas.drawPath(path, Paint()..color = const Color(0xFF00897B));
  }

  @override
  bool shouldRepaint(_Triangle oldDelegate) => false;
}

class _Line extends CustomPainter {
  const _Line();

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawLine(
      Offset(12, size.height - 12),
      Offset(size.width - 12, 12),
      Paint()
        ..color = const Color(0xFF3949AB)
        ..strokeWidth = 8
        ..strokeCap = StrokeCap.round,
    );
  }

  @override
  bool shouldRepaint(_Line oldDelegate) => false;
}
