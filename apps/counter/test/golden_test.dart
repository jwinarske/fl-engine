// The reference frame: the stock engine's flutter_tester, with Impeller,
// renders the counter app, unmodified, at fl-engine's app-lane size.
//
// flutter_tester draws text in its test font unless a test loads real ones,
// and the app ships no Roboto; fl-engine's run is given the same two files
// with `run-app --font`. The platform is Linux, as fl-engine reports it, so
// both use the same typography. DPR 3, flutter_tester's engine DPR, which
// the button's shadow reads.
//
//     flutter test --enable-impeller --update-goldens test/golden_test.dart
import 'dart:io';

import 'package:counter/main.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

Future<void> _load(String family, String file) async {
  final String root = Platform.environment['FLUTTER_ROOT'] ?? '';
  final bytes = File('$root/bin/cache/artifacts/material_fonts/$file').readAsBytesSync();
  await (FontLoader(family)..addFont(Future<ByteData>.value(ByteData.sublistView(bytes)))).load();
}

void main() {
  testWidgets('counter at 1200x900, DPR 3', (tester) async {
    await tester.runAsync(() async {
      await _load('Roboto', 'Roboto-Regular.ttf');
      await _load('MaterialIcons', 'MaterialIcons-Regular.otf');
    });
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    debugDisableShadows = false;
    // The DEBUG banner is debug mode's; fl-engine runs the release build.
    WidgetsApp.debugAllowBannerOverride = false;
    await tester.pumpWidget(const MyApp());
    await expectLater(find.byType(MyApp), matchesGoldenFile('goldens/counter.png'));
    debugDisableShadows = true;
    WidgetsApp.debugAllowBannerOverride = true;
    debugDefaultTargetPlatformOverride = null;
  });
}
