// NetworkImage over HTTPS, as Wonderous loads its photos: dart:io's client
// and TLS, the bytes decoded by the engine. The server is on loopback, with
// a certificate the test makes for localhost and names in FL_TLS_DIR; the
// app's client trusts it alone. A client trusting the system's roots, as
// a plain HttpClient does, refuses it; a missing file is a 404.
import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

class _TrustTestCertificate extends HttpOverrides {
  _TrustTestCertificate(this.context);

  final SecurityContext context;

  @override
  HttpClient createHttpClient(SecurityContext? _) => super.createHttpClient(context);
}

Future<HttpServer> _serve(String dir, Uint8List photo) async {
  final SecurityContext context = SecurityContext()
    ..useCertificateChain('$dir/cert.pem')
    ..usePrivateKey('$dir/key.pem');
  final HttpServer server = await HttpServer.bindSecure(InternetAddress.loopbackIPv4, 0, context);
  server.listen((HttpRequest request) {
    if (request.uri.path == '/photo.jpg') {
      request.response.headers.contentType = ContentType('image', 'jpeg');
      request.response.add(photo);
    } else {
      request.response.statusCode = HttpStatus.notFound;
    }
    request.response.close();
  });
  return server;
}

/// What resolving `provider` comes to: its size, or the error's type.
Future<String> _resolve(ImageProvider provider) {
  final Completer<String> done = Completer<String>();
  final ImageStream stream = provider.resolve(ImageConfiguration.empty);
  late final ImageStreamListener listener;
  listener = ImageStreamListener(
    (ImageInfo info, bool _) {
      done.complete('${info.image.width}x${info.image.height}');
      stream.removeListener(listener);
    },
    onError: (Object error, StackTrace? _) {
      done.complete(error is NetworkImageLoadException ? 'status ${error.statusCode}' : '${error.runtimeType}');
      stream.removeListener(listener);
    },
  );
  stream.addListener(listener);
  return done.future;
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final String dir = Platform.environment['FL_TLS_DIR']!;
  final ByteData photo = await rootBundle.load('assets/photo.jpg');
  final HttpServer server = await _serve(dir, photo.buffer.asUint8List(photo.offsetInBytes, photo.lengthInBytes));
  final String base = 'https://localhost:${server.port}';

  // The system's roots, which the test's certificate is not among.
  final HttpClient system = HttpClient();
  try {
    await (await system.getUrl(Uri.parse('$base/photo.jpg'))).close();
    print('system roots: accepted');
  } on HandshakeException catch (e) {
    print('system roots: refused, ${e.osError?.message.split(':').first.trim()}');
  } finally {
    system.close(force: true);
  }

  HttpOverrides.global = _TrustTestCertificate(SecurityContext(withTrustedRoots: false)..setTrustedCertificates('$dir/cert.pem'));
  final NetworkImage image = NetworkImage('$base/photo.jpg');
  print('photo: ${await _resolve(image)}');
  print('missing: ${await _resolve(NetworkImage('$base/missing.jpg'))}');
  runApp(Center(child: Image(image: image, width: 200, height: 200, fit: BoxFit.fill)));
}
