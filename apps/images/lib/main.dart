// Images decoded and drawn through dart:ui (docs/m3-images-text.md §8). The
// scene is in images.dart so test/golden_test.dart renders the same tree;
// what is printed here is fl-engine's own check, with the refusals the
// review asks for (§15).
import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import 'images.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final images = await decodeAll(rootBundle, print);
  Future<void> refused(String what, Uint8List bytes) async {
    try {
      final codec = await ui.instantiateImageCodec(bytes);
      await codec.getNextFrame();
      print('$what: decoded');
    } catch (e) {
      print('$what: $e');
    }
  }

  await refused('garbage', Uint8List.fromList(List.filled(64, 7)));
  // GIF89a, a 65535x65535 screen, one 1x1 frame: a decompression bomb's
  // header.
  await refused(
    'bomb',
    Uint8List.fromList([
      0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0xff, 0xff, 0xff, 0xff, 0x80, 0, 0, //
      0, 0, 0, 255, 255, 255, //
      0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x4c, 0x01, 0, 0x3b,
    ]),
  );
  // A PNG cut off after its header.
  final png = await rootBundle.load('assets/checker.png');
  await refused('truncated', png.buffer.asUint8List(0, 40));
  // The rest of the image natives (criterion 5): a descriptor's bytes per
  // pixel, a codec's repetition count, an image's color space,
  // toByteData's refusal, and each kind disposed.
  final checker = await rootBundle.load('assets/checker.png');
  final buffer = await ui.ImmutableBuffer.fromUint8List(checker.buffer.asUint8List());
  final descriptor = await ui.ImageDescriptor.encoded(buffer);
  final codec = await descriptor.instantiateCodec();
  final frame = await codec.getNextFrame();
  print(
    'descriptor: ${descriptor.width}x${descriptor.height} bpp=${descriptor.bytesPerPixel} '
    'repeat=${codec.repetitionCount} ${frame.image.colorSpace}',
  );
  try {
    await frame.image.toByteData();
    print('toByteData: done');
  } catch (e) {
    print('toByteData: $e');
  }
  final identity = Float64List.fromList([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
  ui.ImageShader(frame.image, ui.TileMode.clamp, ui.TileMode.clamp, identity).dispose();
  frame.image.dispose();
  codec.dispose();
  descriptor.dispose();
  buffer.dispose();
  // The badge through the framework's own path, AssetImage and the image
  // cache, resolved before the first frame so the widget finds it there:
  // every frame has it, however long the decode took.
  final loaded = Completer<void>();
  const AssetImage('assets/badge.png')
      .resolve(ImageConfiguration.empty)
      .addListener(ImageStreamListener((_, _) => loaded.complete()));
  await loaded.future;
  print('badge.png: cached');
  runApp(Images(images: images));
}
