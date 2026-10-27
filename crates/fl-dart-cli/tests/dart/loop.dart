// run_loop must drain microtasks and timers, not just return after main.
import 'dart:async';

void main(List<String> args) {
  scheduleMicrotask(() => print('microtask'));
  Timer(const Duration(milliseconds: 20), () => print('timer'));
  Future(() => print('future')).then((_) => print('args=${args.join(",")}'));
  print('main');
}
