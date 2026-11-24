// The textures program's scene, shared by main.dart and
// test/golden_test.dart: what shows the texture is the caller's, a Texture
// widget under fl-engine and the same pixels as an image in the golden test.
import 'dart:typed_data';

import 'package:flutter/widgets.dart';

/// The texture's side, in texels.
const int side = 64;

const List<Color> _palette = <Color>[
  Color(0xFFE53935),
  Color(0xFFFFB300),
  Color(0xFF43A047),
  Color(0xFF1E88E5),
  Color(0xFF8E24AA),
  Color(0xFF00ACC1),
  Color(0xFFFFFFFF),
  Color(0xFF212121),
];

/// The producer's pattern at [phase], RGBA8: four by four cells of 16 texels,
/// the cell at column x, row y `_palette[(x + 4 * y + 5 * phase) % 8]`.
Uint8List pattern(int phase) {
  final Uint8List out = Uint8List(side * side * 4);
  var i = 0;
  for (var py = 0; py < side; py++) {
    for (var px = 0; px < side; px++) {
      final Color c = _palette[(px ~/ 16 + 4 * (py ~/ 16) + 5 * phase) % 8];
      out[i++] = (c.r * 255).round();
      out[i++] = (c.g * 255).round();
      out[i++] = (c.b * 255).round();
      out[i++] = (c.a * 255).round();
    }
  }
  return out;
}

/// Two views of one texture: scaled up with nearest sampling, square; and
/// stretched with linear sampling, turned, faded and clipped to a rounded
/// rectangle, so the texture goes through the layers above it.
class Textures extends StatelessWidget {
  const Textures({super.key, required this.view});

  /// The texture, at [FilterQuality], filling the box it is given.
  final Widget Function(FilterQuality quality) view;

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFECEFF1),
        child: Stack(
          children: <Widget>[
            Positioned(left: 40, top: 40, width: 256, height: 256, child: view(FilterQuality.none)),
            Positioned(
              left: 360,
              top: 120,
              width: 360,
              height: 270,
              child: Transform.rotate(
                angle: 0.12,
                child: Opacity(
                  opacity: 0.8,
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(32),
                    child: view(FilterQuality.low),
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
