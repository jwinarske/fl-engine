// A list for fl-engine's app lane to scroll from a script of input events
// (test/scroll.txt): by a mouse's wheel, dragged and flung by a finger,
// panned on a touchpad. Where each scroll leaves it is held against where
// flutter_tester's is left by the same (test/scroll_test.dart,
// docs/m4-input.md exit criterion 4).
import 'package:flutter/widgets.dart';

import 'list.dart';

void main() {
  runApp(const ScrollLog(log: print));
}
