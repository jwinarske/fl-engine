// dart:ui's advanced blend modes, which a board's blend unit lacks and
// emblema then blends in the shader. The scene is in blend_modes.dart so
// test/golden_test.dart renders the same tree.
import 'package:flutter/widgets.dart';

import 'blend_modes.dart';

void main() => runApp(const BlendModes());
