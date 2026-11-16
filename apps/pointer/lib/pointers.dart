// Every pointer event the framework sees, a line each, as a `Listener`
// over the whole view is told them: what the engine's packet converter
// made of the embedder's events, the adds, hovers and removes it
// synthesizes among them. In pointers.dart so test/pointer_test.dart logs
// the same lines under flutter_tester.
import 'package:flutter/gestures.dart';
import 'package:flutter/widgets.dart';

String _n(double v) => v.toStringAsFixed(1);

String _at(Offset o) => '${_n(o.dx)},${_n(o.dy)}';

/// One event as a line: what it is, the kind of device, its buttons,
/// where, how far it moved, and what is its own (a scroll's distance, a
/// gesture's pan, scale and rotation).
String describe(PointerEvent e) {
  // A widget's event is the router's, transformed to the widget's place.
  final String type = e.runtimeType.toString().replaceFirst('_Transformed', '');
  final StringBuffer line = StringBuffer(
    '$type kind=${e.kind.name} buttons=${e.buttons} down=${e.down} '
    'at=${_at(e.position)} delta=${_at(e.delta)}',
  );
  if (e is PointerScrollEvent) {
    line.write(' scroll=${_at(e.scrollDelta)}');
  }
  if (e is PointerPanZoomUpdateEvent) {
    line.write(' pan=${_at(e.pan)} scale=${_n(e.scale)} rotation=${_n(e.rotation)}');
  }
  return line.toString();
}

/// A view that is all `Listener`, and tells `log` each event.
class PointerLog extends StatelessWidget {
  const PointerLog({super.key, required this.log});

  final void Function(String line) log;

  void _event(PointerEvent e) => log(describe(e));

  @override
  Widget build(BuildContext context) {
    return Listener(
      behavior: HitTestBehavior.opaque,
      onPointerDown: _event,
      onPointerMove: _event,
      onPointerUp: _event,
      onPointerHover: _event,
      onPointerCancel: _event,
      onPointerPanZoomStart: _event,
      onPointerPanZoomUpdate: _event,
      onPointerPanZoomEnd: _event,
      onPointerSignal: _event,
      child: const ColoredBox(color: Color(0xFF2196F3), child: SizedBox.expand()),
    );
  }
}

/// Adds and removes are the router's alone: no widget is told of them.
void logRoute(void Function(String line) log) {
  GestureBinding.instance.pointerRouter.addGlobalRoute((PointerEvent e) {
    if (e is PointerAddedEvent || e is PointerRemovedEvent) {
      log(describe(e));
    }
  });
}
