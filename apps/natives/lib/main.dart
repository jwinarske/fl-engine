// The dart:ui natives no other app reaches (docs/m2-dart-ui.md §11,
// criterion 5), each called at least once. The widget is in natives.dart
// so test/golden_test.dart renders the same tree.
import 'dart:ui';

import 'package:flutter/widgets.dart';

import 'natives.dart';

void main() {
  // SetSemanticsTreeEnabled: the framework calls it only when the platform
  // turns semantics on, which fl-shell does not. Off, as it already is.
  PlatformDispatcher.instance.setSemanticsTreeEnabled(false);
  // RequestDartPerformanceMode: the framework asks for latency around a
  // route's transition, which this app has none of. Asked and put back.
  PlatformDispatcher.instance.requestDartPerformanceMode(DartPerformanceMode.latency);
  PlatformDispatcher.instance.requestDartPerformanceMode(DartPerformanceMode.balanced);
  runApp(const Natives());
}
