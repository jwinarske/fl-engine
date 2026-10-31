// The gif program's cases (docs/m3-images-text.md §8), shared by main.dart
// and the tests so both engines decode and draw the same.
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// The animations the report and the sheet decode.
const List<String> animations = ['dispose.gif', 'once.gif', 'anim.webp', 'steps.gif'];

/// Each animation's frame and repetition counts, then every frame's size
/// and duration, one call past the last to show the first comes again.
Future<List<String>> report(AssetBundle bundle) async {
  final lines = <String>[];
  for (final name in animations) {
    final data = await bundle.load('assets/$name');
    final codec = await ui.instantiateImageCodec(data.buffer.asUint8List());
    lines.add('$name: frames=${codec.frameCount} repeat=${codec.repetitionCount}');
    for (var i = 0; i <= codec.frameCount; i++) {
      final frame = await codec.getNextFrame();
      lines.add('  $i: ${frame.image.width}x${frame.image.height} ${frame.duration.inMilliseconds}ms');
      frame.image.dispose();
    }
    codec.dispose();
  }
  return lines;
}

/// Every frame of the first three animations, by name, in order.
Future<Map<String, List<ui.Image>>> decodeFrames(AssetBundle bundle) async {
  final frames = <String, List<ui.Image>>{};
  for (final name in animations.take(3)) {
    final data = await bundle.load('assets/$name');
    final codec = await ui.instantiateImageCodec(data.buffer.asUint8List());
    frames[name] = [for (var i = 0; i < codec.frameCount; i++) (await codec.getNextFrame()).image];
    codec.dispose();
  }
  return frames;
}

/// The animated image, as an app shows one: `Image.asset`, the framework
/// asking for a frame when each is due.
const AssetImage steps = AssetImage('assets/steps.gif');

/// 400x300 logical, gray: each animation's frames in a column, four times
/// their size, unfiltered; the steps animation at the top right, at
/// (340, 10), once loaded.
class Sheet extends StatelessWidget {
  const Sheet({super.key, required this.frames});

  final Map<String, List<ui.Image>> frames;

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFF808080),
        child: Stack(
          children: [
            CustomPaint(size: const Size(400, 300), painter: _Frames(frames)),
            const Positioned(
              left: 340,
              top: 10,
              child: Image(image: steps, width: 40, height: 40),
            ),
          ],
        ),
      ),
    );
  }
}

class _Frames extends CustomPainter {
  _Frames(this.frames);

  final Map<String, List<ui.Image>> frames;

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()..filterQuality = FilterQuality.none;
    // x, and each frame's step down.
    const columns = {'dispose.gif': (10.0, 50.0), 'once.gif': (150.0, 50.0), 'anim.webp': (250.0, 90.0)};
    for (final MapEntry(key: name, value: (x, step)) in columns.entries) {
      final images = frames[name] ?? const [];
      for (var i = 0; i < images.length; i++) {
        final image = images[i];
        final src = Rect.fromLTWH(0, 0, image.width.toDouble(), image.height.toDouble());
        final dst = Rect.fromLTWH(x, 10 + i * step, image.width * 4, image.height * 4);
        canvas.drawImageRect(image, src, dst, paint);
      }
    }
  }

  @override
  bool shouldRepaint(_Frames old) => old.frames != frames;
}
