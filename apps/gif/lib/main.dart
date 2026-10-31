// Animated GIF and WebP frames through Codec.getNextFrame (docs/m3-images-
// text.md §8, exit criterion 4). What is printed is held against the stock
// engine's (test/report.txt); the sheet is the golden, in the first frame
// the steps animation at the top right shows in. The steps are the
// framework's: Image.asset loads them, when depending on the machine, then
// asks for a frame as each is due, which fl-engine's app lane times from
// the frames it renders.
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
