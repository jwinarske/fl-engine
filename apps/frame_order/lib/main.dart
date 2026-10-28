// The engine's frame sequence, seen from dart:ui with no framework in the
// way: ScheduleFrame, then at vsync onBeginFrame, the microtasks it queued,
// onDrawFrame, and the microtasks that queued. Three frames, each asked for
// by the one before; each renders an empty scene so the run counts it.
import 'dart:async';
import 'dart:ui';

int _frame = 0;

void main() {
  final dispatcher = PlatformDispatcher.instance;
  dispatcher.onBeginFrame = (Duration time) {
    _frame++;
    print('begin $_frame');
    scheduleMicrotask(() => print('microtask after begin $_frame'));
  };
  dispatcher.onDrawFrame = () {
    print('draw $_frame');
    scheduleMicrotask(() => print('microtask after draw $_frame'));
    final builder = SceneBuilder();
    final scene = builder.build();
    dispatcher.implicitView!.render(scene);
    scene.dispose();
    if (_frame < 3) {
      dispatcher.scheduleFrame();
    }
  };
  dispatcher.scheduleFrame();
}
