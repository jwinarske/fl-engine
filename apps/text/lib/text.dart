// Text drawn every way a paragraph paints (docs/m3-images-text.md stage 5),
// shared by main.dart and test/golden_test.dart so both engines draw the
// same tree. Roboto and MaterialIcons come from the Flutter cache, as the
// metrics program's do, so neither engine falls back to a system font.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

const String _roboto = 'Roboto';

/// Roboto's regular, bold and italic, and MaterialIcons, from the cache.
Future<void> loadFonts() async {
  final String root = Platform.environment['FLUTTER_ROOT'] ?? '';
  final String dir = '$root/bin/cache/artifacts/material_fonts';
  for (final String f in <String>['Roboto-Regular.ttf', 'Roboto-Bold.ttf', 'Roboto-Italic.ttf']) {
    await ui.loadFontFromList(File('$dir/$f').readAsBytesSync(), fontFamily: _roboto);
  }
  await ui.loadFontFromList(File('$dir/MaterialIcons-Regular.otf').readAsBytesSync(), fontFamily: 'MaterialIcons');
}

class TextSheet extends StatelessWidget {
  const TextSheet({super.key});

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFFFFFFF),
        child: CustomPaint(painter: _Sheet(), size: Size.infinite),
      ),
    );
  }
}

TextStyle _s({
  double size = 14,
  FontWeight? weight,
  FontStyle? style,
  Color color = const Color(0xFF202020),
  Paint? foreground,
  Paint? background,
  TextDecoration? decoration,
  TextDecorationStyle? decorationStyle,
  Color? decorationColor,
  double? decorationThickness,
  List<Shadow>? shadows,
  double? letterSpacing,
  double? wordSpacing,
}) => TextStyle(
  fontFamily: _roboto,
  fontSize: size,
  fontWeight: weight,
  fontStyle: style,
  color: foreground == null ? color : null,
  foreground: foreground,
  background: background,
  decoration: decoration,
  decorationStyle: decorationStyle,
  decorationColor: decorationColor,
  decorationThickness: decorationThickness,
  shadows: shadows,
  letterSpacing: letterSpacing,
  wordSpacing: wordSpacing,
);

class _Sheet extends CustomPainter {
  void _text(Canvas c, Offset at, String s, TextStyle style, {double width = 380, TextAlign align = TextAlign.left, TextDirection dir = TextDirection.ltr, int? maxLines, String? ellipsis}) {
    final TextPainter p = TextPainter(
      text: TextSpan(text: s, style: style),
      textDirection: dir,
      textAlign: align,
      maxLines: maxLines,
      ellipsis: ellipsis,
    )..layout(minWidth: width, maxWidth: width);
    p.paint(c, at);
    p.dispose();
  }

  @override
  void paint(Canvas c, Size size) {
    // Left column: sizes, weights, color, spacing, alignment, direction.
    _text(c, const Offset(10, 8), 'Roboto 10 regular', _s(size: 10));
    _text(c, const Offset(10, 24), 'Roboto 14 regular', _s());
    _text(c, const Offset(10, 44), 'Roboto 20 bold', _s(size: 20, weight: FontWeight.bold));
    _text(c, const Offset(10, 72), 'Roboto 28 italic', _s(size: 28, style: FontStyle.italic));
    _text(c, const Offset(10, 110), 'Colored text', _s(size: 18, color: const Color(0xFF1565C0)));
    _text(c, const Offset(10, 136), 'Letter spacing', _s(size: 16, letterSpacing: 3));
    _text(c, const Offset(10, 160), 'Word spacing set wide', _s(size: 16, wordSpacing: 12));
    _text(c, const Offset(10, 186), 'Centered', _s(size: 16), align: TextAlign.center);
    _text(c, const Offset(10, 208), 'Right aligned', _s(size: 16), align: TextAlign.right);
    _text(c, const Offset(10, 230), 'Right to left, start aligned', _s(size: 16), dir: TextDirection.rtl);
    _text(c, const Offset(10, 254), 'The quick brown fox jumps over the lazy dog and keeps going.', _s(size: 16), maxLines: 1, ellipsis: '…');
    _text(c, const Offset(10, 280),
        'Wrapped across lines: the quick brown fox jumps over the lazy dog, and then over it again.',
        _s(size: 14), width: 220);
    // A glyph past the atlas: drawn as its outline.
    _text(c, const Offset(40, 340), 'Ag', _s(size: 160, weight: FontWeight.bold, color: const Color(0xFF6A1B9A)), width: 340);
    // Right column: decorations, backgrounds, shadows, a stroke (drawn as
    // outlines), an icon. No gradient fill: dart:ui's Gradient natives are
    // not in fl-ui yet.
    _text(c, const Offset(410, 8), 'Underline solid', _s(size: 16, decoration: TextDecoration.underline));
    _text(c, const Offset(410, 32), 'Overline double', _s(size: 16, decoration: TextDecoration.overline, decorationStyle: TextDecorationStyle.double));
    _text(c, const Offset(410, 56), 'Line through dotted', _s(size: 16, decoration: TextDecoration.lineThrough, decorationStyle: TextDecorationStyle.dotted));
    _text(c, const Offset(410, 80), 'Underline dashed red', _s(size: 16, decoration: TextDecoration.underline, decorationStyle: TextDecorationStyle.dashed, decorationColor: const Color(0xFFD32F2F)));
    _text(c, const Offset(410, 104), 'Underline wavy thick', _s(size: 16, decoration: TextDecoration.underline, decorationStyle: TextDecorationStyle.wavy, decorationThickness: 2));
    _text(c, const Offset(410, 132), 'Background', _s(size: 18, background: Paint()..color = const Color(0xFFFFEB3B)));
    _text(c, const Offset(410, 160), 'Hard shadow', _s(size: 20, shadows: const <Shadow>[Shadow(color: Color(0xFF90A4AE), offset: Offset(2, 2))]));
    _text(c, const Offset(410, 192), 'Blurred shadow', _s(size: 20, shadows: const <Shadow>[Shadow(color: Color(0x80000000), offset: Offset(0, 3), blurRadius: 4)]));
    _text(c, const Offset(410, 230), 'Stroked', _s(size: 28, weight: FontWeight.bold,
        foreground: Paint()..style = PaintingStyle.stroke..strokeWidth = 1.5..color = const Color(0xFF2E7D32)));
    _text(c, const Offset(410, 306), String.fromCharCode(Icons.favorite.codePoint) + String.fromCharCode(Icons.star.codePoint) + String.fromCharCode(Icons.home.codePoint),
        const TextStyle(fontFamily: 'MaterialIcons', fontSize: 40, color: Color(0xFFC62828)));
    _text(c, const Offset(410, 360), 'Mixed ', _s(size: 16));
    final TextPainter mixed = TextPainter(
      text: TextSpan(style: _s(size: 16), children: <InlineSpan>[
        const TextSpan(text: 'plain, '),
        TextSpan(text: 'bold, ', style: _s(size: 16, weight: FontWeight.bold)),
        TextSpan(text: 'italic, ', style: _s(size: 16, style: FontStyle.italic)),
        TextSpan(text: 'larger', style: _s(size: 24, color: const Color(0xFF00897B))),
      ]),
      textDirection: TextDirection.ltr,
    )..layout(maxWidth: 380);
    mixed.paint(c, const Offset(410, 384));
    mixed.dispose();
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
