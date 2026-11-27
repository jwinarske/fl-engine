// A code asset, built by native_add's hook and staged in the bundle by
// `flutter build bundle`, bound by @Native through the bundle's
// NativeAssetsManifest.json: from the root isolate, from one compute
// spawns in its group, and a symbol the library does not have. An asset
// id the manifest does not map still resolves in the process. First, before
// the asset is loaded: the library opened by its bare name, which a stock
// bundle's engine finds in lib/ by its RUNPATH, as fl-dart run from a
// bundle's root does by its own.
import 'dart:ffi';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:native_add/native_add.dart';

@Native<Int32 Function()>(symbol: 'getpid', assetId: 'package:native_assets/unmapped.dart')
external int _getpid();

int _addInCompute(int x) => native_add(x, 3);

String _bareOpen() {
  try {
    DynamicLibrary.open('libnative_add.so');
    return 'found';
  } on ArgumentError {
    return 'not found';
  }
}

Future<void> main() async {
  print('bare name: ${_bareOpen()}');
  print('native_add: ${native_add(40, 2)}');
  print('native_add in compute: ${await compute(_addInCompute, 4)}');
  try {
    nativeMissing();
  } on ArgumentError catch (e) {
    final String m = '${e.message}';
    print('missing symbol: ${m.contains("Failed to lookup symbol 'native_missing'")}');
  }
  print('unmapped id from the process: ${_getpid() > 0}');
  runApp(const ColoredBox(color: Color(0xFF2196F3)));
}
