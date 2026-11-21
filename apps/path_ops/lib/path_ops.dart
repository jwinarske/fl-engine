// The path ops program's scene, shared by main.dart and
// test/golden_test.dart so both engines draw the same tree.
import 'dart:math' as math;

import 'package:flutter/widgets.dart';

const _cell = 95.0;

/// The pairs combined, each in a 95 square: what each tests.
final List<(Path, Path)> _pairs = <(Path, Path)>[
  // A circle and a square: curves against lines, overlapping.
  (
    Path()..addOval(const Rect.fromLTWH(10, 10, 55, 55)),
    Path()..addRect(const Rect.fromLTWH(35, 35, 50, 50)),
  ),
  // Wonderous's photo gallery: a rectangle less a rounded window.
  (
    Path()..addRect(const Rect.fromLTWH(5, 5, 85, 85)),
    Path()..addRRect(RRect.fromLTRBR(25, 20, 70, 75, const Radius.circular(6))),
  ),
  // A five-pointed star, self-crossing, even-odd (a hollow pentagon), and
  // a nonzero ellipse.
  (_star()..fillType = PathFillType.evenOdd, Path()..addOval(const Rect.fromLTWH(30, 15, 55, 35))),
  // Cubics and a quadratic: a blob against a lens.
  (
    Path()
      ..moveTo(15, 50)
      ..cubicTo(15, 5, 80, 5, 80, 45)
      ..cubicTo(80, 90, 40, 70, 15, 50)
      ..close(),
    Path()
      ..moveTo(30, 85)
      ..quadraticBezierTo(48, 20, 66, 85)
      ..close(),
  ),
];

Path _star() {
  final path = Path();
  for (var i = 0; i < 5; i++) {
    final a = -math.pi / 2 + i * 4 * math.pi / 5;
    final p = Offset(47.5 + 40 * math.cos(a), 50 + 40 * math.sin(a));
    i == 0 ? path.moveTo(p.dx, p.dy) : path.lineTo(p.dx, p.dy);
  }
  return path..close();
}

const _colors = <Color>[
  Color(0xFF1E88E5),
  Color(0xFFE53935),
  Color(0xFF43A047),
  Color(0xFF8E24AA),
  Color(0xFFFB8C00),
];

/// Rows 0 to 3: a pair each, its five operations across, then the two
/// paths outlined over each other. Row 4: Wonderous's cutout, a clip over
/// a gradient, at five window sizes.
class PathOps extends StatelessWidget {
  const PathOps({super.key});

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFFFFFFF),
        child: Stack(
          children: <Widget>[
            Positioned.fill(child: CustomPaint(painter: _Ops())),
            for (var i = 0; i < 5; i++)
              Positioned(
                left: i * 100.0 + 2.5,
                top: 4 * 100.0 + 2.5,
                width: _cell,
                height: _cell,
                child: ClipPath(
                  clipper: _Cutout(Size(20.0 + i * 15, 60.0 - i * 8)),
                  child: const DecoratedBox(
                    decoration: BoxDecoration(
                      gradient: LinearGradient(colors: <Color>[Color(0xFF263238), Color(0xFF80CBC4)]),
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _Ops extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    for (final (row, (a, b)) in _pairs.indexed) {
      for (final (col, op) in PathOperation.values.indexed) {
        canvas.save();
        canvas.translate(col * 100.0 + 2.5, row * 100.0 + 2.5);
        canvas.drawRect(Offset.zero & const Size(_cell, _cell), Paint()..color = const Color(0xFFF1F1F1));
        canvas.drawPath(Path.combine(op, a, b), Paint()..color = _colors[col]);
        canvas.restore();
      }
      canvas.save();
      canvas.translate(5 * 100.0 + 2.5, row * 100.0 + 2.5);
      final outline = Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.5;
      canvas.drawPath(a, outline..color = const Color(0xFF1E88E5));
      canvas.drawPath(b, outline..color = const Color(0xFFE53935));
      canvas.restore();
    }
  }

  @override
  bool shouldRepaint(_Ops oldDelegate) => false;
}

/// Wonderous's `_CutoutClipper`: the whole box less a rounded window in
/// its middle.
class _Cutout extends CustomClipper<Path> {
  _Cutout(this.window);

  final Size window;

  @override
  Path getClip(Size size) {
    final padX = (size.width - window.width) / 2;
    final padY = (size.height - window.height) / 2;
    return Path.combine(
      PathOperation.difference,
      Path()..addRect(Offset.zero & size),
      Path()..addRRect(RRect.fromLTRBR(padX, padY, size.width - padX, size.height - padY, const Radius.circular(6))),
    );
  }

  @override
  bool shouldReclip(_Cutout oldClipper) => oldClipper.window != window;
}

