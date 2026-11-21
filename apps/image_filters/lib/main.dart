// dart:ui's image filters (docs/wonderous.md): blur, dilate, erode, a color
// filter and compositions, on a paint, through saveLayer, as a layer, and
// on a backdrop. The scene is in image_filters.dart so
// test/golden_test.dart renders the same tree.
import 'package:flutter/widgets.dart';

import 'image_filters.dart';

void main() => runApp(const ImageFilters());
