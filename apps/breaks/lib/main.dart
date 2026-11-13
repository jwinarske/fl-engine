// Word and line breaks printed, for fl-engine's app lane to hold against
// the stock engine's (test/breaks.txt). The cases are in breaks.dart so
// test/breaks_test.dart runs the same ones under flutter_tester.
import 'package:flutter/widgets.dart';

import 'breaks.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await loadFonts();
  for (final String line in probe()) {
    print(line);
  }
  runApp(const ColoredBox(color: Color(0xFF2196F3)));
}
