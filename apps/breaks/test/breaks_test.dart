// The reference breaks: the stock engine's flutter_tester, with Impeller,
// runs the same cases.
//
//     flutter test --enable-impeller --update-goldens
import 'dart:io';

import 'package:breaks/breaks.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('breaks', () async {
    await loadFonts();
    final String out = '${probe().join('\n')}\n';
    final File expected = File('test/breaks.txt');
    if (autoUpdateGoldenFiles) {
      expected.writeAsStringSync(out);
    } else {
      expect(out, expected.readAsStringSync());
    }
  });
}
