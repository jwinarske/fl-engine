// Text that changes every frame (docs/m3-images-text.md §10, L5): each
// frame lays out new strings at a size it has not used for 64 frames, so
// the glyph atlas takes new glyphs every frame and must evict to stay
// bounded. Every frame is asked for by the ticker. Run with
// `--font Roboto=...`: the family is named, so nothing falls back.
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';

void main() => runApp(const Churn());

const List<String> _words = [
  'quick', 'brown', 'fox', 'jumps', 'over', 'lazy', 'dog', 'pack', 'my',
  'box', 'with', 'five', 'dozen', 'liquor', 'jugs', 'sphinx', 'black',
  'quartz', 'judge', 'vow', 'WALTZ', 'NYMPH', 'FJORD', 'GLYPH', '0123456789',
];

class Churn extends StatefulWidget {
  const Churn({super.key});

  @override
  State<Churn> createState() => _ChurnState();
}

class _ChurnState extends State<Churn> with SingleTickerProviderStateMixin {
  late final Ticker _ticker;
  var _frame = 0;

  @override
  void initState() {
    super.initState();
    _ticker = createTicker((_) => setState(() => _frame++))..start();
  }

  @override
  void dispose() {
    _ticker.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final n = _frame;
    // 64 sizes, 8 to 71 px, visited in a scrambled order.
    final size = 8.0 + (n * 37) % 64;
    final words = [for (var i = 0; i < 6; i++) _words[(n * 7 + i * 11) % _words.length]];
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFFFFFFFF),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'frame $n',
              style: const TextStyle(fontFamily: 'Roboto', fontSize: 14, color: Color(0xFF000000)),
            ),
            Text(
              words.join(' '),
              style: TextStyle(fontFamily: 'Roboto', fontSize: size, color: const Color(0xFF202080)),
            ),
            Text(
              words.reversed.join(' '),
              style: TextStyle(
                fontFamily: 'Roboto',
                fontSize: 80 - size / 2,
                fontStyle: n.isEven ? FontStyle.normal : FontStyle.italic,
                color: const Color(0xFF802020),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
