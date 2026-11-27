// The asset id is this library's URI, the one the hook names
// (package:native_add/src/native_add.dart).
import 'dart:ffi';

@Native<Int32 Function(Int32, Int32)>(isLeaf: true)
external int native_add(int a, int b);

// In the library, but not exported by it: the asset is found and the
// lookup in it fails.
@Native<Int32 Function()>(symbol: 'native_missing')
external int nativeMissing();
