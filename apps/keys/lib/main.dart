// Prints every key event the framework sees, for fl-engine's app lane to
// hold against what flutter_tester logs for the same keys (test/typing.txt
// and test/keys_test.dart, docs/m4-input.md §9, D5). The `data` lines are
// the engine's own message, which flutter_tester does not send.
//
// ignore_for_file: deprecated_member_use
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import 'keys.dart';

void main() {
  runApp(const ColoredBox(color: Color(0xFF2196F3)));
  KeyLog(print).attach();
  RawKeyboard.instance.addListener((RawKeyEvent e) => print(rawData(e)));
}
