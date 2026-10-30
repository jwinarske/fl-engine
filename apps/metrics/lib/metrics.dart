// Fixed paragraphs in Roboto, laid out and asked every query dart:ui has.
// Roboto comes from the Flutter cache, as the text goldens' does, so both
// engines lay out the same faces; nothing here needs a fallback font.
import 'dart:io';
import 'dart:ui' as ui;

const String _family = 'Roboto';

/// Roboto's regular, bold and italic, registered as 'Roboto'.
Future<void> loadFonts() async {
  final String root = Platform.environment['FLUTTER_ROOT'] ?? '';
  final String dir = '$root/bin/cache/artifacts/material_fonts';
  for (final String f in <String>['Roboto-Regular.ttf', 'Roboto-Bold.ttf', 'Roboto-Italic.ttf']) {
    await ui.loadFontFromList(File('$dir/$f').readAsBytesSync(), fontFamily: _family);
  }
}

String _n(double v) {
  if (v.isInfinite || v.abs() > 1e30) {
    return v.isNegative ? '-inf' : 'inf';
  }
  final String s = v.toStringAsFixed(3);
  return s == '-0.000' ? '0.000' : s;
}

String _box(ui.TextBox b) =>
    '[${_n(b.left)} ${_n(b.top)} ${_n(b.right)} ${_n(b.bottom)} ${b.direction.name}]';

String _boxes(List<ui.TextBox> bs) => bs.map(_box).join(' ');

ui.ParagraphStyle _ps({
  ui.TextAlign textAlign = ui.TextAlign.start,
  ui.TextDirection textDirection = ui.TextDirection.ltr,
  int? maxLines,
  String? ellipsis,
  double fontSize = 14,
  double? height,
  ui.TextHeightBehavior? textHeightBehavior,
  ui.StrutStyle? strutStyle,
}) => ui.ParagraphStyle(
  fontFamily: _family,
  fontSize: fontSize,
  textAlign: textAlign,
  textDirection: textDirection,
  maxLines: maxLines,
  ellipsis: ellipsis,
  height: height,
  textHeightBehavior: textHeightBehavior,
  strutStyle: strutStyle,
);

ui.TextStyle _ts({
  double fontSize = 14,
  ui.FontWeight? fontWeight,
  ui.FontStyle? fontStyle,
  double? letterSpacing,
  double? wordSpacing,
  double? height,
  ui.TextLeadingDistribution? leadingDistribution,
  List<ui.FontFeature>? fontFeatures,
  ui.TextBaseline? textBaseline,
  ui.Locale? locale,
}) => ui.TextStyle(
  color: const ui.Color(0xFF000000),
  fontFamily: _family,
  fontSize: fontSize,
  fontWeight: fontWeight,
  fontStyle: fontStyle,
  letterSpacing: letterSpacing,
  wordSpacing: wordSpacing,
  height: height,
  leadingDistribution: leadingDistribution,
  fontFeatures: fontFeatures,
  textBaseline: textBaseline,
  locale: locale,
);

class Spec {
  Spec(this.name, this.width, this.text, {ui.ParagraphStyle? style, ui.TextStyle? textStyle, this.extra})
    : style = style ?? _ps(),
      textStyle = textStyle ?? _ts();
  final String name;
  final double width;
  final String text;
  final ui.ParagraphStyle style;
  final ui.TextStyle textStyle;
  // Builds the paragraph's content itself when set.
  final void Function(ui.ParagraphBuilder b)? extra;
}

const String _long = 'The quick brown fox jumps over the lazy dog, and then it sleeps.';

final List<Spec> specs = <Spec>[
  Spec('plain', 400, 'Hello world'),
  Spec('wrap', 120, _long),
  Spec('wrap-narrow', 30, _long),
  Spec('unbounded', double.infinity, _long),
  Spec('newlines', 300, 'one\ntwo\n\nfour\n'),
  Spec('empty', 100, ''),
  Spec('trailing-spaces', 300, 'Hello   '),
  Spec('leading-spaces', 300, '   Hello'),
  Spec('tab', 300, 'a\tb\tc'),
  Spec('long-word', 50, 'Supercalifragilisticexpialidocious word'),
  Spec('size-30-bold', 300, 'Bold and big', textStyle: _ts(fontSize: 30, fontWeight: ui.FontWeight.bold)),
  Spec('italic', 300, 'Leaning text', textStyle: _ts(fontStyle: ui.FontStyle.italic)),
  Spec('height-1.5', 150, _long, textStyle: _ts(height: 1.5)),
  // Its own text: Skia's ParagraphCache keys a paragraph without its
  // leading distribution, so with height-1.5's text the stock engine
  // answers with height-1.5's proportional runs.
  Spec(
    'height-1.5-even',
    150,
    'Even leading puts half the extra height above the text and half below.',
    textStyle: _ts(height: 1.5, leadingDistribution: ui.TextLeadingDistribution.even),
  ),
  Spec('height-0.5', 150, _long, textStyle: _ts(height: 0.5)),
  Spec('letter-spacing', 200, 'Spaced out letters', textStyle: _ts(letterSpacing: 2)),
  Spec('word-spacing', 200, 'Spaced out words here', textStyle: _ts(wordSpacing: 6)),
  Spec('negative-spacing', 200, 'Tight letters', textStyle: _ts(letterSpacing: -1)),
  Spec('max-lines-2', 120, _long, style: _ps(maxLines: 2)),
  Spec('ellipsis', 120, _long, style: _ps(maxLines: 2, ellipsis: '…')),
  Spec('ellipsis-one', 100, _long, style: _ps(maxLines: 1, ellipsis: '...')),
  Spec('center', 200, _long, style: _ps(textAlign: ui.TextAlign.center)),
  Spec('right', 200, _long, style: _ps(textAlign: ui.TextAlign.right)),
  Spec('justify', 200, _long, style: _ps(textAlign: ui.TextAlign.justify)),
  Spec('end', 200, _long, style: _ps(textAlign: ui.TextAlign.end)),
  Spec('rtl', 200, _long, style: _ps(textDirection: ui.TextDirection.rtl)),
  Spec('rtl-justify', 200, _long, style: _ps(textDirection: ui.TextDirection.rtl, textAlign: ui.TextAlign.justify)),
  Spec(
    'strut',
    150,
    _long,
    style: _ps(strutStyle: ui.StrutStyle(fontFamily: _family, fontSize: 20, height: 1.2)),
  ),
  Spec(
    'strut-forced',
    150,
    _long,
    style: _ps(
      strutStyle: ui.StrutStyle(fontFamily: _family, fontSize: 10, height: 1, forceStrutHeight: true, leading: 0.5),
    ),
  ),
  Spec(
    'disable-ascent-descent',
    150,
    _long,
    textStyle: _ts(height: 2),
    style: _ps(
      textHeightBehavior: const ui.TextHeightBehavior(
        applyHeightToFirstAscent: false,
        applyHeightToLastDescent: false,
      ),
    ),
  ),
  Spec('tabular', 300, '0123 1111 7777', textStyle: _ts(fontFeatures: <ui.FontFeature>[const ui.FontFeature.tabularFigures()])),
  Spec('no-liga', 300, 'office fifty', textStyle: _ts(fontFeatures: <ui.FontFeature>[const ui.FontFeature.disable('liga')])),
  Spec('locale', 300, 'Hello world', textStyle: _ts(locale: const ui.Locale('en', 'US'))),
  Spec('accents', 300, 'Crème brûlée, é naïve'),
  Spec('greek-cyrillic', 300, 'Αθήνα Москва'),
  Spec(
    'mixed',
    300,
    'Plain bold big italic',
    extra: (ui.ParagraphBuilder b) {
      b.pushStyle(_ts());
      b.addText('Plain ');
      b.pushStyle(_ts(fontWeight: ui.FontWeight.bold));
      b.addText('bold ');
      b.pushStyle(_ts(fontSize: 24));
      b.addText('big ');
      b.pop();
      b.pop();
      b.pushStyle(_ts(fontStyle: ui.FontStyle.italic));
      b.addText('italic');
    },
  ),
  Spec(
    'placeholders',
    200,
    'ab￼cd￼ef￼',
    extra: (ui.ParagraphBuilder b) {
      b.pushStyle(_ts());
      b.addText('ab');
      b.addPlaceholder(20, 10, ui.PlaceholderAlignment.middle);
      b.addText('cd');
      b.addPlaceholder(30, 24, ui.PlaceholderAlignment.baseline, baseline: ui.TextBaseline.alphabetic, baselineOffset: 18);
      b.addText('ef');
      b.addPlaceholder(15, 40, ui.PlaceholderAlignment.top);
    },
  ),
  Spec(
    'placeholder-aligns',
    400,
    '￼a￼b￼c￼',
    extra: (ui.ParagraphBuilder b) {
      b.pushStyle(_ts());
      b.addPlaceholder(10, 30, ui.PlaceholderAlignment.aboveBaseline, baseline: ui.TextBaseline.alphabetic);
      b.addText('a');
      b.addPlaceholder(10, 30, ui.PlaceholderAlignment.belowBaseline, baseline: ui.TextBaseline.alphabetic);
      b.addText('b');
      b.addPlaceholder(10, 30, ui.PlaceholderAlignment.bottom);
      b.addText('c');
      b.addPlaceholder(10, 30, ui.PlaceholderAlignment.baseline, baseline: ui.TextBaseline.ideographic, baselineOffset: 20);
    },
  ),
  Spec('placeholder-wrap', 60, 'word ￼ word', extra: (ui.ParagraphBuilder b) {
    b.pushStyle(_ts());
    b.addText('word ');
    b.addPlaceholder(50, 20, ui.PlaceholderAlignment.middle);
    b.addText(' word');
  }),
];

/// A paragraph style and two text styles with every field set, each to a
/// value of its own, built and laid out. fl-engine's `--styles` prints what
/// it decoded, held against test/styles.txt.
List<String> everyField() {
  final ui.ParagraphBuilder b = ui.ParagraphBuilder(
    ui.ParagraphStyle(
      textAlign: ui.TextAlign.right,
      textDirection: ui.TextDirection.rtl,
      fontWeight: ui.FontWeight.w700,
      fontStyle: ui.FontStyle.italic,
      maxLines: 3,
      fontFamily: _family,
      fontSize: 15,
      height: 1.25,
      textHeightBehavior: const ui.TextHeightBehavior(
        applyHeightToFirstAscent: false,
        leadingDistribution: ui.TextLeadingDistribution.even,
      ),
      strutStyle: ui.StrutStyle(
        fontFamily: _family,
        fontFamilyFallback: <String>['Fallback'],
        fontSize: 13,
        height: 1.1,
        leadingDistribution: ui.TextLeadingDistribution.even,
        leading: 0.2,
        fontWeight: ui.FontWeight.w300,
        fontStyle: ui.FontStyle.italic,
        forceStrutHeight: true,
      ),
      ellipsis: '..',
      locale: const ui.Locale('fr', 'CA'),
    ),
  );
  b.pushStyle(
    ui.TextStyle(
      color: const ui.Color(0xFF112233),
      decoration: ui.TextDecoration.combine(<ui.TextDecoration>[
        ui.TextDecoration.underline,
        ui.TextDecoration.lineThrough,
      ]),
      decorationColor: const ui.Color(0xFF445566),
      decorationStyle: ui.TextDecorationStyle.dashed,
      decorationThickness: 2.5,
      fontWeight: ui.FontWeight.w600,
      fontStyle: ui.FontStyle.italic,
      textBaseline: ui.TextBaseline.ideographic,
      fontFamily: _family,
      fontFamilyFallback: <String>['Fallback'],
      fontSize: 17,
      letterSpacing: 1.5,
      wordSpacing: 3.5,
      height: 1.75,
      leadingDistribution: ui.TextLeadingDistribution.even,
      locale: const ui.Locale('de', 'DE'),
      background: ui.Paint()..color = const ui.Color(0xFF778899),
      shadows: <ui.Shadow>[
        const ui.Shadow(color: ui.Color(0x80102030), offset: ui.Offset(1, 2), blurRadius: 3),
      ],
      fontFeatures: <ui.FontFeature>[const ui.FontFeature('smcp'), const ui.FontFeature('tnum', 2)],
      fontVariations: <ui.FontVariation>[const ui.FontVariation('wght', 500), const ui.FontVariation('wdth', 90)],
    ),
  );
  b.addText('Every field ');
  b.pushStyle(ui.TextStyle(foreground: ui.Paint()..color = const ui.Color(0xFFAABBCC), fontSize: 11));
  b.addText('set');
  final ui.Paragraph p = b.build()..layout(const ui.ParagraphConstraints(width: 300));
  final List<String> out = <String>['every-field: lines=${p.numberOfLines} height=${_n(p.height)} longest=${_n(p.longestLine)} max=${_n(p.maxIntrinsicWidth)} alphabetic=${_n(p.alphabeticBaseline)}'];
  p.dispose();
  return out;
}

/// Microseconds each paragraph took to build and lay out, in all.
int _layoutMicros = 0;

ui.Paragraph _build(Spec s) {
  final Stopwatch w = Stopwatch()..start();
  final ui.ParagraphBuilder b = ui.ParagraphBuilder(s.style);
  if (s.extra != null) {
    s.extra!(b);
  } else {
    b.pushStyle(s.textStyle);
    b.addText(s.text);
  }
  final ui.Paragraph p = b.build();
  p.layout(ui.ParagraphConstraints(width: s.width));
  _layoutMicros += w.elapsedMicroseconds;
  return p;
}

/// Every spec's measures and query answers, a line each.
List<String> measure() {
  final List<String> out = <String>[];
  for (final Spec s in specs) {
    final ui.Paragraph p = _build(s);
    final String t = s.name;
    final int len = s.text.length;
    out.add(
      '$t: width=${_n(p.width)} height=${_n(p.height)} longest=${_n(p.longestLine)} '
      'min=${_n(p.minIntrinsicWidth)} max=${_n(p.maxIntrinsicWidth)} '
      'alphabetic=${_n(p.alphabeticBaseline)} ideographic=${_n(p.ideographicBaseline)} '
      'exceeded=${p.didExceedMaxLines} lines=${p.numberOfLines}',
    );
    for (final ui.LineMetrics m in p.computeLineMetrics()) {
      out.add(
        '$t line ${m.lineNumber}: hard=${m.hardBreak} ascent=${_n(m.ascent)} descent=${_n(m.descent)} '
        'unscaled=${_n(m.unscaledAscent)} height=${_n(m.height)} width=${_n(m.width)} '
        'left=${_n(m.left)} baseline=${_n(m.baseline)}',
      );
    }
    final ui.LineMetrics? first = p.getLineMetricsAt(0);
    out.add('$t lineAt 0: ${first == null ? 'none' : '${_n(first.baseline)} ${_n(first.width)}'}');
    for (final ui.BoxHeightStyle h in ui.BoxHeightStyle.values) {
      out.add('$t boxes ${h.name}: ${_boxes(p.getBoxesForRange(0, len, boxHeightStyle: h))}');
    }
    out.add('$t boxes max-width: ${_boxes(p.getBoxesForRange(0, len, boxWidthStyle: ui.BoxWidthStyle.max))}');
    if (len > 4) {
      out.add('$t boxes 2..${len - 2}: ${_boxes(p.getBoxesForRange(2, len - 2))}');
    }
    out.add('$t placeholders: ${_boxes(p.getBoxesForPlaceholders())}');
    final List<String> pos = <String>[];
    for (final ui.Offset o in <ui.Offset>[
      const ui.Offset(-5, -5),
      const ui.Offset(0, 0),
      const ui.Offset(3, 5),
      const ui.Offset(25, 8),
      const ui.Offset(60, 20),
      const ui.Offset(110, 40),
      const ui.Offset(1000, 1000),
    ]) {
      final ui.TextPosition tp = p.getPositionForOffset(o);
      pos.add('${tp.offset}${tp.affinity == ui.TextAffinity.upstream ? 'u' : 'd'}');
    }
    out.add('$t positions: ${pos.join(' ')}');
    final List<String> words = <String>[];
    final List<String> lines = <String>[];
    final List<String> lineNumbers = <String>[];
    final List<String> glyphs = <String>[];
    for (int i = 0; i <= len; i += len > 12 ? 5 : 1) {
      final ui.TextRange w = p.getWordBoundary(ui.TextPosition(offset: i));
      words.add('$i:${w.start}-${w.end}');
      final ui.TextRange l = p.getLineBoundary(ui.TextPosition(offset: i));
      lines.add('$i:${l.start}-${l.end}');
      lineNumbers.add('$i:${p.getLineNumberAt(i) ?? '-'}');
      final ui.GlyphInfo? g = p.getGlyphInfoAt(i);
      glyphs.add(
        g == null
            ? '$i:none'
            : '$i:${g.graphemeClusterCodeUnitRange.start}-${g.graphemeClusterCodeUnitRange.end}'
                  '${g.writingDirection.name[0]}'
                  '(${_n(g.graphemeClusterLayoutBounds.left)},${_n(g.graphemeClusterLayoutBounds.top)},'
                  '${_n(g.graphemeClusterLayoutBounds.right)},${_n(g.graphemeClusterLayoutBounds.bottom)})',
      );
    }
    out.add('$t words: ${words.join(' ')}');
    out.add('$t lineBoundaries: ${lines.join(' ')}');
    out.add('$t lineNumbers: ${lineNumbers.join(' ')}');
    out.add('$t glyphs: ${glyphs.join(' ')}');
    final ui.GlyphInfo? closest = p.getClosestGlyphInfoForOffset(const ui.Offset(30, 6));
    out.add(
      '$t closest: ${closest == null ? 'none' : '${closest.graphemeClusterCodeUnitRange.start}-${closest.graphemeClusterCodeUnitRange.end}'}',
    );
    p.dispose();
  }
  return out;
}

/// The layout time, which the comparison leaves out.
String timing() => 'time: ${specs.length} paragraphs, ${(_layoutMicros / specs.length).round()} us each';
