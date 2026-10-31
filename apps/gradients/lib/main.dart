// dart:ui's gradients (docs/m3-status.md): each kind, tile mode and
// matrix. The scene is in gradients.dart so test/golden_test.dart renders
// the same tree.
import 'package:flutter/widgets.dart';

import 'gradients.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(Gradients(image: await quadrants()));
}
