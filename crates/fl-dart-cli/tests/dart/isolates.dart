// Isolates in the program's own group, and Uri.base: what Flutter's
// `compute` and package:path need of the embedder.
import 'dart:io';
import 'dart:isolate';

Future<void> main() async {
  print('base=${Uri.base == Uri.directory(Directory.current.path)}');

  print('run=${await Isolate.run(() => 6 * 7)}');

  // print from a spawned isolate reaches the same sink.
  await Isolate.run(() => print('printed from a spawned isolate'));

  final ReceivePort port = ReceivePort();
  await Isolate.spawn((SendPort reply) => reply.send('spawned'), port.sendPort);
  print('spawn=${await port.first}');

  // An exception in a spawned isolate comes back to the caller.
  try {
    await Isolate.run(() => throw StateError('thrown in a spawned isolate'));
  } on StateError catch (e) {
    print('error=${e.message}');
  }

  // A new group is still refused, and says so.
  try {
    await Isolate.spawnUri(Uri.parse('other.dart'), <String>[], null);
  } on IsolateSpawnException {
    print('spawnUri refused');
  }
}
