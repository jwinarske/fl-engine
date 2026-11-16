// A TextField for fl-engine's app lane to type into from a script of input
// events (test/edit.txt): text, a dead key's composed character, a
// selection dragged with the mouse, a copy and a paste, Return. Its frames
// are held against flutter_tester's, and the editing values it prints
// against what flutter_tester logs for the same edits
// (test/text_field_test.dart, docs/m4-input.md §9, D5).
//
// `fl/seed` puts text in the field from outside, for a run that starts
// with some: `fl-dart run-app --message fl/seed=...`.
import 'package:flutter/material.dart';

import 'field.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  // The caret is drawn, and does not blink: a frame is the same whenever
  // it is taken.
  EditableText.debugDeterministicCursor = true;
  final TextEditingController controller = TextEditingController();
  listenForSeed(controller);
  runApp(TextFieldApp(log: print, controller: controller));
}
