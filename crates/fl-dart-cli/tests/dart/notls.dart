// On the VM without secure sockets: a TLS connection is refused with
// ArgumentError, the plain socket it had opened is closed, and the program
// ends. The server here is a plain one, so each connection itself succeeds;
// it prints when its end sees the other close.
import 'dart:async';
import 'dart:io';

Future<void> main() async {
  final server = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
  var closed = Completer<void>();
  server.listen((peer) {
    peer.listen((_) {}, onDone: () {
      peer.destroy();
      closed.complete();
    }, onError: (_) {});
  });

  try {
    await SecureSocket.connect(InternetAddress.loopbackIPv4, server.port);
    print('socket: connected');
  } on ArgumentError catch (e) {
    print('socket: ${e.message}');
  }
  await closed.future;
  print('socket: closed');

  closed = Completer<void>();
  final client = HttpClient();
  try {
    await client.getUrl(Uri.parse('https://127.0.0.1:${server.port}/'));
    print('https: requested');
  } on ArgumentError catch (e) {
    print('https: ${e.message}');
  }
  await closed.future;
  print('https: closed');
  client.close(force: true);

  try {
    SecurityContext().useCertificateChainBytes(const [0]);
    print('context: took a certificate');
  } on ArgumentError catch (e) {
    print('context: ${e.message}');
  }
  await server.close();
  print('done');
}
