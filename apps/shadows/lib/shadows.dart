// The shadows program's scene, shared by main.dart and
// test/golden_test.dart so both engines draw the same tree.
import 'package:flutter/widgets.dart';

/// Cells 100x100 logical, four across: drawShadow under each occluder, the
/// occluder drawn over it as Material draws it.
class Shadows extends StatelessWidget {
  const Shadows({super.key});

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFF4F4F4),
        child: Stack(
          children: [
            CustomPaint(size: const Size(400, 200), painter: _Cells()),
            // The framework's own caller: PhysicalModel, as Material uses.
            const Positioned(
              left: 20,
              top: 220,
              child: PhysicalModel(
                color: Color(0xFF6750A4),
                elevation: 6,
                shadowColor: Color(0xFF000000),
                borderRadius: BorderRadius.all(Radius.circular(16)),
                child: SizedBox(width: 56, height: 56),
              ),
            ),
            const Positioned(
              left: 120,
              top: 220,
              child: PhysicalModel(
                color: Color(0xFFFFFFFF),
                elevation: 1,
                shadowColor: Color(0xFF000000),
                shape: BoxShape.circle,
                child: SizedBox(width: 56, height: 56),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Cells extends CustomPainter {
  static Rect _box(int i) => Rect.fromLTWH((i % 4) * 100 + 20, (i ~/ 4) * 100 + 20, 60, 50);

  @override
  void paint(Canvas canvas, Size size) {
    final white = Paint()..color = const Color(0xFFFFFFFF);
    // Rects at rising elevation, black at Material's shadow alpha.
    for (final (i, e) in [(0, 1.0), (1, 3.0), (2, 8.0), (3, 16.0)]) {
      final p = Path()..addRect(_box(i));
      canvas.drawShadow(p, const Color(0xFF000000), e, false);
      canvas.drawPath(p, white);
    }
    // A rounded rect, an oval, a colored shadow, a transparent occluder.
    final rr = Path()..addRRect(RRect.fromRectAndRadius(_box(4), const Radius.circular(12)));
    canvas.drawShadow(rr, const Color(0xFF000000), 6, false);
    canvas.drawPath(rr, white);
    final oval = Path()..addOval(_box(5));
    canvas.drawShadow(oval, const Color(0xFF000000), 6, false);
    canvas.drawPath(oval, white);
    final blue = Path()..addRect(_box(6));
    canvas.drawShadow(blue, const Color(0xFF1040E0), 6, false);
    canvas.drawPath(blue, white);
    final clear = Path()..addRect(_box(7));
    canvas.drawShadow(clear, const Color(0x80000000), 6, true);
    canvas.drawPath(clear, Paint()..color = const Color(0x40FFFFFF));
  }

  @override
  bool shouldRepaint(_Cells old) => false;
}
