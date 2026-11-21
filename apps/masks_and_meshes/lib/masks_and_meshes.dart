// The masks and meshes program's scene, shared by main.dart and
// test/golden_test.dart so both engines draw the same tree.
import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

const _red = Color(0xFFE53935);
const _yellow = Color(0xFFFDD835);
const _green = Color(0xFF43A047);
const _blue = Color(0xFF1E88E5);

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

/// Cells 95x95 logical, eight across. Rows 0 and 1: `ShaderMask` over a
/// sample, a shader and a blend mode a cell. Rows 2 and 3: `drawVertices`.
class MasksAndMeshes extends StatelessWidget {
  const MasksAndMeshes({super.key, required this.image});

  final ui.Image image;

  static Offset _at(int i) => Offset((i % 8) * 100.0 + 2.5, (i ~/ 8) * 100.0 + 2.5);

  @override
  Widget build(BuildContext context) {
    final fade = const LinearGradient(colors: <Color>[Color(0xFFFFFFFF), Color(0x00FFFFFF)]);
    final rainbow = const LinearGradient(colors: <Color>[_red, _yellow, _green, _blue]);
    final glow = const RadialGradient(colors: <Color>[Color(0xFFFFFFFF), Color(0x00FFFFFF)]);
    // flutter_animate's shimmer: a band across, srcATop.
    final shimmer = const LinearGradient(
      begin: Alignment(-1, -0.3),
      end: Alignment(1, 0.3),
      colors: <Color>[Color(0x00FFFFFF), Color(0xC0FFFFFF), Color(0x00FFFFFF)],
      stops: <double>[0.35, 0.5, 0.65],
    );
    // Only modes that keep transparent black so: Impeller clips a shader
    // mask's layer to what its children painted (`ShaderMaskLayer::Paint`,
    // `saveLayer(paint_bounds())`), and fl-engine has no picture bounds to
    // clip it to, so a mode that colors nothing (multiply) differs outside
    // the children (docs/wonderous.md).
    final masks = <(ShaderCallback, BlendMode)>[
      ((r) => fade.createShader(r), BlendMode.dstIn),
      ((r) => rainbow.createShader(r), BlendMode.srcIn),
      ((r) => rainbow.createShader(r), BlendMode.modulate),
      ((r) => shimmer.createShader(r), BlendMode.srcATop),
      ((r) => glow.createShader(r), BlendMode.dstIn),
      ((r) => const RadialGradient(colors: <Color>[_red, _blue]).createShader(r), BlendMode.srcIn),
      (
        (r) => ui.ImageShader(
          image,
          TileMode.clamp,
          TileMode.clamp,
          Matrix4.diagonal3Values(r.width / 4, r.height / 4, 1).storage,
        ),
        BlendMode.srcATop,
      ),
      ((r) => fade.createShader(r), BlendMode.dstOut),
      ((r) => rainbow.createShader(r), BlendMode.srcATop),
      (
        (r) => const LinearGradient(
          colors: <Color>[_blue, _red],
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
        ).createShader(r),
        BlendMode.srcIn,
      ),
    ];
    return Directionality(
      textDirection: TextDirection.ltr,
      child: Stack(
        children: <Widget>[
          const ColoredBox(color: Color(0xFFFFFFFF), child: SizedBox(width: 800, height: 600)),
          for (var j = 0; j < masks.length; j++)
            Positioned.fromRect(
              rect: _at(j) & const Size(95, 95),
              child: ShaderMask(shaderCallback: masks[j].$1, blendMode: masks[j].$2, child: const _Sample()),
            ),
          Positioned.fromRect(
            rect: const Offset(0, 200) & const Size(800, 200),
            child: CustomPaint(painter: _Meshes(image)),
          ),
        ],
      ),
    );
  }
}

/// What a mask is over: a red square, a yellow circle and a blue bar.
class _Sample extends StatelessWidget {
  const _Sample();

  @override
  Widget build(BuildContext context) => CustomPaint(painter: _SamplePainter());
}

class _SamplePainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final r = Offset.zero & size;
    canvas.drawRect(r.deflate(10), Paint()..color = _red);
    canvas.drawCircle(r.center, 26, Paint()..color = _yellow);
    canvas.drawRect(Rect.fromLTWH(r.left + 5, r.center.dy - 5, r.width - 10, 10), Paint()..color = _blue);
  }

  @override
  bool shouldRepaint(_SamplePainter oldDelegate) => false;
}

class _Meshes extends CustomPainter {
  _Meshes(this.image);

  final ui.Image image;

  static Offset _at(int i) => Offset((i % 8) * 100.0 + 2.5, (i ~/ 8) * 100.0 + 2.5);

  /// Four corners of cell `i`, inset, and its center.
  static List<Offset> _quad(int i) {
    final o = _at(i);
    return <Offset>[o + const Offset(8, 8), o + const Offset(87, 8), o + const Offset(87, 87), o + const Offset(8, 87)];
  }

  static const _corners = <Color>[_red, _yellow, _green, _blue];

  @override
  void paint(Canvas canvas, Size size) {
    void draw(int i, ui.Vertices v, BlendMode mode, Paint paint) {
      canvas.drawVertices(v, mode, paint);
      v.dispose();
    }

    final p = _quad;
    // Triangles, a strip and a fan, one color.
    draw(0, ui.Vertices(VertexMode.triangles, [p(0)[0], p(0)[1], p(0)[3]]), BlendMode.srcOver, Paint()..color = _blue);
    draw(
      1,
      ui.Vertices(VertexMode.triangleStrip, [p(1)[0], p(1)[1], p(1)[3], p(1)[2]]),
      BlendMode.srcOver,
      Paint()..color = _green,
    );
    final c = _at(2) + const Offset(47.5, 47.5);
    draw(
      2,
      ui.Vertices(VertexMode.triangleFan, [c, for (var k = 0; k < 4; k++) p(2)[k], p(2)[0]]),
      BlendMode.srcOver,
      Paint()..color = _red,
    );
    // Indexed, colors at the corners under each blend mode.
    final indices = <int>[0, 1, 2, 0, 2, 3];
    for (final (i, mode) in <(int, BlendMode)>[
      (3, BlendMode.dst),
      (4, BlendMode.modulate),
      (5, BlendMode.srcOver),
      (6, BlendMode.multiply),
    ]) {
      draw(
        i,
        ui.Vertices(VertexMode.triangles, p(i), colors: _corners, indices: indices),
        mode,
        Paint()..color = const Color(0x80FFFFFF),
      );
    }
    // A gradient read at texture coordinates, and the image.
    final uv = <Offset>[Offset.zero, const Offset(95, 0), const Offset(95, 95), const Offset(0, 95)];
    draw(
      7,
      ui.Vertices(VertexMode.triangles, p(7), textureCoordinates: uv, indices: indices),
      BlendMode.srcOver,
      Paint()..shader = ui.Gradient.linear(Offset.zero, const Offset(95, 95), <Color>[_red, _blue]),
    );
    final imageUv = <Offset>[Offset.zero, const Offset(4, 0), const Offset(4, 4), const Offset(0, 4)];
    draw(
      8,
      ui.Vertices(VertexMode.triangles, p(8), textureCoordinates: imageUv, indices: indices),
      BlendMode.srcOver,
      Paint()..shader = ui.ImageShader(image, TileMode.clamp, TileMode.clamp, Matrix4.identity().storage),
    );
    // The image with colors over it, and a mesh at half the paint's alpha.
    draw(
      9,
      ui.Vertices(VertexMode.triangles, p(9), textureCoordinates: imageUv, colors: _corners, indices: indices),
      BlendMode.modulate,
      Paint()..shader = ui.ImageShader(image, TileMode.clamp, TileMode.clamp, Matrix4.identity().storage),
    );
    draw(
      10,
      ui.Vertices(VertexMode.triangles, p(10), indices: indices),
      BlendMode.srcOver,
      Paint()..color = _blue.withValues(alpha: 0.5),
    );
  }

  @override
  bool shouldRepaint(_Meshes oldDelegate) => false;
}
