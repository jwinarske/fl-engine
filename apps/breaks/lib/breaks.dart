// Where the engine breaks words and lines, for text that tells ICU's
// break iterators (the stock engine's) from UAX #14 and #29 as written.
// Widths only need to agree with themselves: each probe measures with the
// engine it runs on, so a fallback font or a missing glyph changes no
// answer.
import 'dart:io';
import 'dart:ui' as ui;

const String _family = 'Roboto';

/// Roboto's regular, registered as 'Roboto'.
Future<void> loadFonts() async {
  final String root = Platform.environment['FLUTTER_ROOT'] ?? '';
  final String dir = '$root/bin/cache/artifacts/material_fonts';
  await ui.loadFontFromList(File('$dir/Roboto-Regular.ttf').readAsBytesSync(), fontFamily: _family);
}

/// Each case's name and text.
const List<(String, String)> cases = <(String, String)>[
  ('english', 'The quick brown fox, jumps over the lazy dog.'),
  ('contractions', "don't can't it's o'clock rock'n'roll"),
  ('numbers', r'1,000.50 $12.99 3.14159 -42 +7 50% 1/2 2024-01-15 10:30'),
  ('abbreviations', 'e.g. i.e. U.S.A. Mr. Dr. etc.'),
  ('hyphens', 'co-op state-of-the-art well-known e-mail -5 x-y'),
  ('slashes', 'and/or 1/2 http://example.com/a/b?c=d&e=f'),
  ('email', 'name@example.com first.last+tag@sub.domain.org'),
  ('punctuation', 'Wait... What?! (yes) [no] {maybe} "quoted" “smart” ‘single’'),
  ('dashes', 'one—two three–four five — six'),
  ('spaces', 'nb sp thin sp zw​sp wj⁠x ideo　sp'),
  ('soft-hyphen', 'hyphen­ation super­cali­fragilistic'),
  ('underscore', 'snake_case_name camelCase CONSTANT_VALUE __init__'),
  ('emoji', 'hi \u{1F44B}\u{1F3FD} family \u{1F468}‍\u{1F469}‍\u{1F467} flag \u{1F1EF}\u{1F1F5} ok'),
  ('japanese', '日本語の文章を分割します。東京で会いましょう。'),
  ('japanese-kana', 'カタカナとひらがなー。「かぎ」もある。'),
  ('chinese', '今天天气很好，我们去公园散步吧。'),
  ('korean', '한국어 문장을 나눕니다. 안녕하세요'),
  ('thai', 'ภาษาไทยเขียนติดกันไม่มีช่องว่าง'),
  ('lao', 'ພາສາລາວຂຽນຕິດກັນ'),
  ('khmer', 'ភាសាខ្មែរសរសេរជាប់គ្នា'),
  ('burmese', 'မြန်မာဘာသာစကားကိုရေးသည်'),
  ('arabic', 'اللغة العربية جميلة، أليس كذلك؟'),
  ('hebrew', 'עברית היא שפה יפה, נכון?'),
  ('hindi', 'हिन्दी एक सुंदर भाषा है। क्या?'),
  ('mixed', 'Tokyo東京2024年 abcไทยdef 한국Korea'),
  ('quotes-cjk', '「こんにちは」と言った。（笑）'),
  ('closing-punct', 'end.) next.] (a) "b" «c» ‹d›'),
];

/// The width of `text`'s first `i` code units, laid out on one line.
double _prefixWidth(ui.Paragraph p, int i) {
  double w = 0;
  for (final ui.TextBox b in p.getBoxesForRange(0, i)) {
    w += b.right - b.left;
  }
  return w;
}

ui.Paragraph _build(String text, double width) {
  final ui.ParagraphBuilder b = ui.ParagraphBuilder(ui.ParagraphStyle(fontFamily: _family, fontSize: 14));
  b.pushStyle(ui.TextStyle(color: const ui.Color(0xFF000000), fontFamily: _family, fontSize: 14));
  b.addText(text);
  final ui.Paragraph p = b.build();
  p.layout(ui.ParagraphConstraints(width: width));
  return p;
}

/// Each case's word boundary at every offset, and where its first line
/// ends when it is as wide as each prefix: the last break at or before
/// the prefix's end.
List<String> probe() {
  final List<String> out = <String>[];
  for (final (String name, String text) in cases) {
    final ui.Paragraph one = _build(text, double.infinity);
    final List<String> words = <String>[];
    for (int i = 0; i <= text.length; i++) {
      final ui.TextRange w = one.getWordBoundary(ui.TextPosition(offset: i));
      words.add('$i:${w.start}-${w.end}');
    }
    out.add('$name words: ${words.join(' ')}');
    // Behind a word wider than any of the cases', so every line is wider
    // than the word it wraps: no word is broken at its clusters, where the
    // faces each engine found would decide.
    final String pad = '${'a' * 40} ';
    final String padded = '$pad$text';
    final ui.Paragraph wide = _build(padded, double.infinity);
    final List<String> ends = <String>[];
    for (int i = 1; i <= text.length; i++) {
      final double w = _prefixWidth(wide, i + pad.length);
      final ui.Paragraph p = _build(padded, w + 0.01);
      ends.add('$i:${p.getLineBoundary(const ui.TextPosition(offset: 0)).end - pad.length}');
    }
    out.add('$name ends: ${ends.join(' ')}');
  }
  return out;
}
