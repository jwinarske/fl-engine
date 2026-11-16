// Prints every pointer event the framework sees, for fl-engine's app lane
// to hold against what flutter_tester logs for the same pointers
// (test/pointers.txt and test/pointer_test.dart, docs/m4-input.md §9, D5).
import 'package:flutter/widgets.dart';

import 'pointers.dart';

void main() {
  runApp(const PointerLog(log: print));
  logRoute(print);
}
