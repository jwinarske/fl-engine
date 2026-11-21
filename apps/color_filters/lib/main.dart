// dart:ui's color filters (docs/wonderous.md): each kind on a paint, an
// image, text and a saveLayer, and as a layer. The scene is in
// color_filters.dart so test/golden_test.dart renders the same tree.
import 'package:flutter/widgets.dart';

import 'color_filters.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await loadFonts();
  runApp(ColorFilters(image: await quadrants()));
}
