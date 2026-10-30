// Paragraph layout and its queries, printed, for fl-engine's app lane to
// hold against the stock engine's output (test/metrics.txt), within half
// a pixel. The cases are in metrics.dart so test/metrics_test.dart runs
// the same ones under flutter_tester.
import 'package:flutter/widgets.dart';

import 'metrics.dart';

Future<void> main() async {
  // Each loadFontFromList sends `fontsChange` on flutter/system; the
  // binding listens for it, so none is dropped.
  WidgetsFlutterBinding.ensureInitialized();
  await loadFonts();
  for (final String line in <String>[...measure(), ...everyField()]) {
    print(line);
  }
  print(timing());
  runApp(const ColoredBox(color: Color(0xFF2196F3)));
}
