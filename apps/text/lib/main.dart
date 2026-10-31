// Text drawn every way a paragraph paints, for fl-engine's app lane to hold
// against the stock engine's golden. The tree is in text.dart.
import 'package:flutter/widgets.dart';

import 'text.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await loadFonts();
  runApp(const TextSheet());
}
