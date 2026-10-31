// The gradients program's scene, shared by main.dart and
// test/golden_test.dart so both engines draw the same tree.
import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

const _red = Color(0xFFE53935);
const _yellow = Color(0xFFFDD835);
const _green = Color(0xFF43A047);
const _blue = Color(0xFF1E88E5);
const _purple = Color(0xC08E24AA);

/// A 4x4 image, a color a quadrant, for the image shaders: red, green,
/// blue, yellow, clockwise from the top left.
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

/// Cells 95x95 logical, eight across, each filled with one gradient or
/// image shader.
class Gradients extends StatelessWidget {
  const Gradients({super.key, required this.image});

  final ui.Image image;

  @override
  Widget build(BuildContext context) {
    return CustomPaint(size: const Size(800, 600), painter: _Cells(image));
  }
}

class _Cells extends CustomPainter {
  _Cells(this.image);

  final ui.Image image;

  static Rect _cell(int i) => Offset((i % 8) * 100.0 + 2.5, (i ~/ 8) * 100.0 + 2.5) & const Size(95, 95);

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawColor(const Color(0xFFFFFFFF), BlendMode.src);
    var i = 0;
    void fill(ui.Shader shader, {double alpha = 1.0}) {
      final r = _cell(i++);
      canvas.drawRect(r, Paint()..shader = shader..color = Color.fromRGBO(0, 0, 0, alpha));
    }

    Offset at(double x, double y) => _cell(i).topLeft + Offset(x, y);

    // Linear: two stops; many stops (past emblema's four in the material).
    fill(ui.Gradient.linear(at(0, 0), at(95, 0), [_red, _blue]));
    fill(ui.Gradient.linear(at(0, 0), at(95, 95), [_red, _yellow, _green, _blue, _purple, _red],
        [0, 0.1, 0.3, 0.5, 0.8, 1]));
    // Each tile mode, the ramp a third of the cell.
    for (final mode in TileMode.values) {
      fill(ui.Gradient.linear(at(30, 0), at(62, 0), [_red, _yellow, _blue], [0, 0.5, 1], mode));
    }
    // The paint's alpha on the shader.
    fill(ui.Gradient.linear(at(0, 0), at(0, 95), [_green, _blue]), alpha: 0.5);

    // Radial: plain; each tile mode at a small radius.
    fill(ui.Gradient.radial(at(47.5, 47.5), 47.5, [_yellow, _red, _blue], [0, 0.6, 1]));
    for (final mode in TileMode.values) {
      fill(ui.Gradient.radial(at(47.5, 47.5), 15, [_blue, _yellow], null, mode));
    }
    // Conical: a focal point off center; a focal radius.
    fill(ui.Gradient.radial(at(47.5, 47.5), 45, [_yellow, _purple], null, TileMode.clamp, null,
        at(25, 30), 0));
    fill(ui.Gradient.radial(at(60, 60), 35, [_green, _blue, _red], [0, 0.5, 1], TileMode.clamp, null,
        at(30, 30), 10));

    // Sweep: whole turn; part of a turn with each tile mode.
    fill(ui.Gradient.sweep(at(47.5, 47.5), [_red, _yellow, _green, _blue, _red],
        [0, 0.25, 0.5, 0.75, 1]));
    for (final mode in TileMode.values) {
      fill(ui.Gradient.sweep(at(47.5, 47.5), [_blue, _yellow], null, mode, math.pi / 4, math.pi));
    }

    // Matrices: the framework's GradientRotation on a linear gradient; a
    // skewed linear one; a rotated and scaled radial and sweep.
    fill(const LinearGradient(colors: [_red, _blue], transform: GradientRotation(math.pi / 5))
        .createShader(_cell(i)));
    final skew = Matrix4.identity()
      ..translateByDouble(at(0, 0).dx, at(0, 0).dy, 0, 1)
      ..setEntry(0, 1, 0.6)
      ..translateByDouble(-at(0, 0).dx, -at(0, 0).dy, 0, 1);
    fill(ui.Gradient.linear(at(0, 30), at(50, 30), [_green, _purple], null, TileMode.mirror,
        skew.storage));
    final c = at(47.5, 47.5);
    final turn = Matrix4.identity()
      ..translateByDouble(c.dx, c.dy, 0, 1)
      ..rotateZ(0.7)
      ..scaleByDouble(1.5, 1.5, 1, 1)
      ..translateByDouble(-c.dx, -c.dy, 0, 1);
    fill(ui.Gradient.radial(c, 20, [_yellow, _blue], null, TileMode.repeated, turn.storage));
    final c2 = at(47.5, 47.5);
    final turn2 = Matrix4.identity()
      ..translateByDouble(c2.dx, c2.dy, 0, 1)
      ..rotateZ(1.2)
      ..translateByDouble(-c2.dx, -c2.dy, 0, 1);
    fill(ui.Gradient.sweep(c2, [_red, _green, _red], [0, 0.5, 1], TileMode.clamp, 0, math.pi * 2,
        turn2.storage));

    // Image shaders by their matrix: scaled up and turned, mirrored, and
    // skewed, each repeating.
    Float64List placed(void Function(Matrix4) by) {
      final o = _cell(i).topLeft;
      final m = Matrix4.identity()..translateByDouble(o.dx, o.dy, 0, 1);
      by(m);
      return m.storage;
    }

    fill(ui.ImageShader(image, TileMode.repeated, TileMode.repeated,
        placed((m) => m..translateByDouble(47.5, 47.5, 0, 1)..rotateZ(0.5)..scaleByDouble(8, 8, 1, 1)),
        filterQuality: FilterQuality.none));
    fill(ui.ImageShader(image, TileMode.repeated, TileMode.repeated,
        placed((m) => m..scaleByDouble(-10, 10, 1, 1)), filterQuality: FilterQuality.none));
    fill(ui.ImageShader(image, TileMode.mirror, TileMode.mirror,
        placed((m) => m..setEntry(0, 1, 0.5)..scaleByDouble(12, 12, 1, 1)),
        filterQuality: FilterQuality.none));

    // A stroked circle and a rounded rect in a gradient.
    final s = _cell(i++);
    canvas.drawCircle(s.center, 38, Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 12
      ..shader = ui.Gradient.sweep(s.center, [_red, _blue, _red], [0, 0.5, 1]));
    final rr = _cell(i++);
    canvas.drawRRect(RRect.fromRectAndRadius(rr, const Radius.circular(24)),
        Paint()..shader = ui.Gradient.linear(rr.topLeft, rr.bottomRight, [_purple, _yellow]));
  }

  @override
  bool shouldRepaint(_Cells old) => old.image != image;
}
