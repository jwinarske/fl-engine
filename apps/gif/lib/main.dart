// Animated GIF and WebP frames through Codec.getNextFrame (docs/m3-images-
// text.md §8, exit criterion 4). What is printed is held against the stock
// engine's (test/report.txt); the first frame, the sheet, is the golden.
// The steps animation at the top right is the framework's: Image.asset
// loads it after the first frame, then asks for a frame as each is due,
// which fl-engine's app lane times from the frames it renders.
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import 'gif.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  for (final line in await report(rootBundle)) {
    print(line);
  }
  runApp(Sheet(frames: await decodeFrames(rootBundle)));
}
