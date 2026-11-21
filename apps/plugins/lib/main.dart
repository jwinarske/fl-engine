// A Dart-only plugin, bound by the registrant the Flutter tool generates
// before main runs, as shared_preferences_linux is; and compute, which runs
// its callback in a spawned isolate, as flutter_svg parses a picture.
import 'dart:io';
import 'dart:ui' as ui;

import 'package:dart_only_plugin/dart_only_plugin.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

int _square(int x) => x * x;

// dart:ui is the root isolate's: off it, a native is not resolved.
int _record(int _) {
  ui.PictureRecorder();
  return 0;
}

Future<void> main() async {
  print('registered=${DartOnlyPlugin.registered}');
  print('compute=${await compute(_square, 12)}');
  try {
    await compute(_record, 0);
  } on ArgumentError catch (e) {
    print('dart:ui in compute: ${e.message}');
  }
  // Where package_info_plus reads the app's version on Linux: beside the
  // running executable, as a bundle lays it out.
  final File version = File('${File(Platform.resolvedExecutable).parent.path}/data/flutter_assets/version.json');
  print('version.json beside the runner: ${version.existsSync() ? version.readAsStringSync() : 'none'}');
  runApp(const ColoredBox(color: Color(0xFF2196F3)));
}
