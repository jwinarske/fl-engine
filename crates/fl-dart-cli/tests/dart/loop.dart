// run_loop must drain microtasks and timers, not just return after main.
import 'dart:async';

void main(List<String> args) {
  scheduleMicrotask(() => print('microtask'));
  Future(() => print('future')).then((_) {
    print('args=${args.join(",")}');
    // Started here, not in main: Future() is itself a zero-length timer, and
    // two timers both due by the time the loop first runs (a slow start, as
    // under TSan) fire in either order.
    Timer(const Duration(milliseconds: 20), () => print('timer'));
  });
  print('main');
}
