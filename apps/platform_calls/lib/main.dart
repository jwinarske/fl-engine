// What the framework asks the platform for and awaits, answered as the Linux
// embedder answers it: haptics, orientations and the status bar, which it
// does not have, read as null; sounds; the route reports go_router makes on
// flutter/navigation; a plugin's channel with no handler; and leaving, put
// to the app first when it may refuse.
import 'dart:ui' show AppExitResponse, AppExitType;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

Future<void> _say(String what, Future<Object?> Function() call) async {
  try {
    await call();
    print('$what: answered');
  } on MissingPluginException {
    print('$what: no handler');
  }
}

int _asked = 0;

Future<void> _leave() async {
  // The first is refused by the app, the second let through.
  final AppExitResponse first = await ServicesBinding.instance.exitApplication(AppExitType.cancelable);
  print('exit 1: ${first.name}');
  await ServicesBinding.instance.exitApplication(AppExitType.cancelable);
  // Not reached: the loop has returned.
  print('exit 2: still here');
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  AppLifecycleListener(
    onExitRequested: () async {
      _asked += 1;
      print('asked to exit: $_asked');
      return _asked == 1 ? AppExitResponse.cancel : AppExitResponse.exit;
    },
  );
  await _say('haptic', HapticFeedback.lightImpact);
  await _say('orientations', () => SystemChrome.setPreferredOrientations(<DeviceOrientation>[DeviceOrientation.portraitUp]));
  await _say('overlay', () => SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersive));
  await _say('sound', () => SystemSound.play(SystemSoundType.alert));
  await _say('single entry', SystemNavigator.selectSingleEntryHistory);
  await _say('route', () => SystemNavigator.routeInformationUpdated(uri: Uri.parse('/welcome')));
  await _say('plugin', () => const MethodChannel('fl/no_such_plugin').invokeMethod<void>('hello'));
  // url_launcher_linux's two calls, as its pigeon code sends them; with no
  // launcher set, nothing can be opened and the launch says why.
  const String pigeon = 'dev.flutter.pigeon.url_launcher_linux.UrlLauncherApi';
  const BasicMessageChannel<Object?> can = BasicMessageChannel<Object?>('$pigeon.canLaunchUrl', StandardMessageCodec());
  const BasicMessageChannel<Object?> launch = BasicMessageChannel<Object?>('$pigeon.launchUrl', StandardMessageCodec());
  print('url can launch: ${await can.send(<Object?>['https://www.wonderous.info/'])}');
  print('url can launch a custom scheme: ${await can.send(<Object?>['wonderous://open'])}');
  print('url launch: ${await launch.send(<Object?>['https://www.wonderous.info/'])}');
  print('url launch that fails: ${await launch.send(<Object?>['https://fail.example/'])}');
  // The same two calls before url_launcher_linux 3.2.0, on its method
  // channel, as its Dart side made them: a failed launch is raised.
  const MethodChannel legacy = MethodChannel('plugins.flutter.io/url_launcher_linux');
  Future<String> legacyLaunch(String url) async {
    try {
      return '${await legacy.invokeMethod<bool>('launch', url)}';
    } on PlatformException catch (e) {
      return '${e.code}: ${e.message}';
    }
  }

  print('channel can launch: ${await legacy.invokeMethod<bool>('canLaunch', 'https://www.wonderous.info/')}');
  print('channel can launch a custom scheme: ${await legacy.invokeMethod<bool>('canLaunch', 'wonderous://open')}');
  print('channel launch: ${await legacyLaunch('https://www.wonderous.info/')}');
  print('channel launch that fails: ${await legacyLaunch('https://fail.example/')}');
  runApp(const ColoredBox(color: Color(0xFF4CAF50)));
  WidgetsBinding.instance.addPostFrameCallback((_) => _leave());
}
