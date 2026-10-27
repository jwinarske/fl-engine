// Both M1 crossings (brief §9). Uses only dart:ffi: the string is written
// through libc's malloc, looked up the same way as the Rust exports, so the
// program needs no packages.
import 'dart:ffi';
import 'dart:isolate';

final _p = DynamicLibrary.process();

final _add = _p.lookupFunction<Int64 Function(Int64, Int64), int Function(int, int)>('fl_dart_add');
final _utf8Len = _p.lookupFunction<Int64 Function(Pointer<Uint8>), int Function(Pointer<Uint8>)>('fl_dart_utf8_len');
final _post = _p.lookupFunction<Bool Function(Int64, Int64), bool Function(int, int)>('fl_dart_post_from_thread');
final _malloc = _p.lookupFunction<Pointer<Uint8> Function(IntPtr), Pointer<Uint8> Function(int)>('malloc');
final _free = _p.lookupFunction<Void Function(Pointer<Uint8>), void Function(Pointer<Uint8>)>('free');

Pointer<Uint8> _cString(String s) {
  final units = s.codeUnits; // ASCII-only input below; UTF-8 is not the point here
  final p = _malloc(units.length + 1);
  for (var i = 0; i < units.length; i++) {
    p[i] = units[i];
  }
  p[units.length] = 0;
  return p;
}

Future<void> main() async {
  print('add=${_add(40, 2)}');
  final s = _cString('hello, rust');
  print('len=${_utf8Len(s)}');
  _free(s);

  final port = ReceivePort();
  if (!_post(port.sendPort.nativePort, 7)) throw StateError('post thread did not start');
  final msg = await port.first as List;
  print('port=${msg[0]},${msg[1]}');
}
