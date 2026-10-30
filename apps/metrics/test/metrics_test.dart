// The reference numbers: the stock engine's flutter_tester, with Impeller,
// runs the same cases (docs/m3-images-text.md §9).
//
//     flutter test --enable-impeller --update-goldens
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:metrics/metrics.dart';

void main() {
  test('metrics', () async {
    await loadFonts();
    final String out = '${<String>[...measure(), ...everyField()].join('\n')}\n';
    final File expected = File('test/metrics.txt');
    if (autoUpdateGoldenFiles) {
      expected.writeAsStringSync(out);
    } else {
      expect(out, expected.readAsStringSync());
    }
  });
}
