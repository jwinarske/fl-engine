// The blend modes program's scene, shared by main.dart and
// test/golden_test.dart so both engines draw the same tree.
import 'package:flutter/widgets.dart';

/// Every mode no blend unit does by factors alone, one cell each.
const List<BlendMode> modes = <BlendMode>[
  BlendMode.screen,
  BlendMode.overlay,
  BlendMode.darken,
  BlendMode.lighten,
  BlendMode.colorDodge,
  BlendMode.colorBurn,
  BlendMode.hardLight,
  BlendMode.softLight,
  BlendMode.difference,
  BlendMode.exclusion,
  BlendMode.multiply,
  BlendMode.hue,
  BlendMode.saturation,
  BlendMode.color,
  BlendMode.luminosity,
];

const _columns = 5;
const _cellWidth = 160.0;
const _cellHeight = 200.0;

class BlendModes extends StatelessWidget {
  const BlendModes({super.key});

  @override
  Widget build(BuildContext context) {
    return const Directionality(
      textDirection: TextDirection.ltr,
      child: CustomPaint(painter: _Painter(), size: Size(800, 600)),
    );
  }
}

class _Painter extends CustomPainter {
  const _Painter();

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = const Color(0xFFFFFFFF));
    for (var i = 0; i < modes.length; i++) {
      canvas.save();
      canvas.translate((i % _columns) * _cellWidth, (i ~/ _columns) * _cellHeight);
      _cell(canvas, modes[i]);
      canvas.restore();
    }
  }

  /// What is under each draw: a gradient across hues, with a translucent
  /// band below it so the modes meet a destination that is not opaque.
  void _destination(Canvas canvas) {
    const rect = Rect.fromLTWH(8, 8, 144, 184);
    canvas.drawRect(
      rect,
      Paint()
        ..shader = const LinearGradient(colors: <Color>[Color(0xFF1E88E5), Color(0xFFFDD835), Color(0xFFE53935)])
            .createShader(rect),
    );
    canvas.drawRect(const Rect.fromLTWH(8, 150, 144, 42), Paint()..color = const Color(0x8000C853));
  }

  void _cell(Canvas canvas, BlendMode mode) {
    _destination(canvas);
    // A paint with a solid color: the core variant.
    canvas.drawCircle(
      const Offset(48, 52),
      34,
      Paint()
        ..color = const Color(0xCC8E24AA)
        ..blendMode = mode,
    );
    // A rounded rectangle under a gradient: the shape variant.
    const rrect = Rect.fromLTWH(84, 20, 60, 70);
    canvas.drawRRect(
      RRect.fromRectAndRadius(rrect, const Radius.circular(14)),
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: <Color>[Color(0xFF00ACC1), Color(0x40FF7043)],
        ).createShader(rrect)
        ..blendMode = mode,
    );
    // A group composited by the mode: two overlapping shapes, drawn
    // source-over into the layer, the layer blended at three quarters.
    canvas.saveLayer(
      const Rect.fromLTWH(16, 100, 128, 88),
      Paint()
        ..color = const Color(0xC0000000)
        ..blendMode = mode,
    );
    canvas.drawRect(const Rect.fromLTWH(20, 108, 70, 70), Paint()..color = const Color(0xFF43A047));
    canvas.drawOval(const Rect.fromLTWH(60, 120, 80, 60), Paint()..color = const Color(0xFFD81B60));
    canvas.restore();
  }

  @override
  bool shouldRepaint(_Painter oldDelegate) => false;
}
