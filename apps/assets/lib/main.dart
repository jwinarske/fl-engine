// The asset bundle (docs/m3-images-text.md, stage 1): each way the
// framework reaches it, and the ways it must refuse, printed for the test
// to check. Then one frame, so the run counts as rendered.
import 'dart:convert';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // flutter/assets, through the platform message.
  final ByteData hello = await rootBundle.load('assets/hello.txt');
  print('load: ${utf8.decode(hello.buffer.asUint8List()).trim()}');

  // ImmutableBuffer.fromAsset, the native.
  final ui.ImmutableBuffer fromAsset = await ui.ImmutableBuffer.fromAsset('assets/hello.txt');
  print('fromAsset: ${fromAsset.length} bytes');
  fromAsset.dispose();

  // ImmutableBuffer.fromUint8List.
  final ui.ImmutableBuffer fromList = await ui.ImmutableBuffer.fromUint8List(
    Uint8List.fromList(<int>[1, 2, 3]),
  );
  print('fromUint8List: ${fromList.length} bytes');
  fromList.dispose();

  // Not found, and a key that tries to leave the bundle: both a miss.
  for (final String key in <String>['assets/none.txt', '../escape.txt']) {
    try {
      await rootBundle.load(key);
      print('load $key: found');
    } catch (_) {
      print('load $key: not found');
    }
    try {
      await ui.ImmutableBuffer.fromAsset(key);
      print('fromAsset $key: found');
    } catch (_) {
      print('fromAsset $key: not found');
    }
  }

  // A file that is not there, and one that is not a regular file.
  for (final String path in <String>['/nonexistent/file', '/dev/zero']) {
    try {
      await ui.ImmutableBuffer.fromFilePath(path);
      print('fromFilePath $path: read');
    } catch (_) {
      print('fromFilePath $path: refused');
    }
  }

  // FontLoader: completes (fonts register at M3 stage 4).
  final FontLoader loader = FontLoader('Nothing')..addFont(Future<ByteData>.value(hello));
  await loader.load();
  print('FontLoader: done');

  runApp(const ColoredBox(color: Color(0xFF4CAF50)));
}
