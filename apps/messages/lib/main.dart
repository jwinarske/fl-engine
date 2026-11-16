// Messages the engine sends the framework (_dispatchPlatformMessage), seen
// from dart:ui with no framework in the way, and every way one is answered:
// with bytes, with none, with null, late by a listener set after the message
// arrived, by ChannelBuffers dropping it for a newer one, and never.
import 'dart:convert';
import 'dart:typed_data';
import 'dart:ui';

String _text(ByteData? data) =>
    data == null ? 'null' : '"${utf8.decode(data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes))}"';

ByteData _bytes(String text) => ByteData.sublistView(Uint8List.fromList(utf8.encode(text)));

void _listen(String channel, ByteData? Function(ByteData?) answer) {
  channelBuffers.setListener(channel, (ByteData? data, PlatformMessageResponseCallback callback) {
    print('$channel: ${_text(data)}');
    callback(answer(data));
  });
}

// Kept, and never called.
final List<PlatformMessageResponseCallback> _unanswered = <PlatformMessageResponseCallback>[];

void main() {
  _listen('fl/echo', (ByteData? data) => data);
  _listen('fl/empty', (ByteData? data) => ByteData(0));
  _listen('fl/null', (ByteData? data) => null);
  channelBuffers.setListener('fl/never', (ByteData? data, PlatformMessageResponseCallback callback) {
    print('fl/never: ${_text(data)}');
    _unanswered.add(callback);
  });
  channelBuffers.setListener('fl/twice', (ByteData? data, PlatformMessageResponseCallback callback) {
    print('fl/twice: ${_text(data)}');
    callback(_bytes('first'));
    try {
      callback(_bytes('second'));
    } catch (e) {
      print('fl/twice: ${e.runtimeType}');
    }
  });

  final PlatformDispatcher dispatcher = PlatformDispatcher.instance;
  dispatcher.onBeginFrame = (Duration time) {};
  dispatcher.onDrawFrame = () {
    // fl/late's message has been waiting in its channel's buffer since
    // before this frame.
    _listen('fl/late', (ByteData? data) => _bytes('late'));
    final SceneBuilder builder = SceneBuilder();
    final Scene scene = builder.build();
    dispatcher.implicitView!.render(scene);
    scene.dispose();
  };
  dispatcher.scheduleFrame();
}
