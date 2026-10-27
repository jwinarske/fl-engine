// Racing posts: many Rust threads post to one port at once. Every message
// must arrive, and each thread's messages in the order it sent them.
import 'dart:ffi';
import 'dart:isolate';

final _burst = DynamicLibrary.process().lookupFunction<
    Bool Function(Int64, Int64, Int64),
    bool Function(int, int, int)>('fl_dart_post_burst');

const threads = 8;
const each = 1000;

Future<void> main() async {
  final port = ReceivePort();
  final next = List<int>.filled(threads, 0);
  var got = 0;
  final done = port.take(threads * each).forEach((m) {
    final msg = m as List;
    final t = msg[0] as int, seq = msg[1] as int;
    if (seq != next[t]) {
      throw StateError('thread $t: got $seq, expected ${next[t]}');
    }
    next[t]++;
    got++;
  });
  if (!_burst(port.sendPort.nativePort, threads, each)) {
    throw StateError('a poster failed');
  }
  await done;
  port.close();
  print('burst=$got ok');
}
