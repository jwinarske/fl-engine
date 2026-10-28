// MaterialApp with no text (docs/m2-dart-ui.md §8): the widget library's
// own app, navigator and theme over the drawing set. With a restoration
// scope its first frame waits on flutter/restoration's answer
// (RootRestorationScope defers it). The widget is in home.dart so
// test/golden_test.dart renders the same tree.
import 'package:flutter/material.dart';

import 'home.dart';

void main() => runApp(
  const MaterialApp(debugShowCheckedModeBanner: false, restorationScopeId: 'material', home: Home()),
);
