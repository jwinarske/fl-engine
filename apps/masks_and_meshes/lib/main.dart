// dart:ui's shader masks and meshes (docs/wonderous.md): SceneBuilder's
// pushShaderMask, as ShaderMask pushes it, and Canvas.drawVertices. The
// scene is in masks_and_meshes.dart so test/golden_test.dart renders the
// same tree.
import 'package:flutter/widgets.dart';

import 'masks_and_meshes.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(MasksAndMeshes(image: await quadrants()));
}
