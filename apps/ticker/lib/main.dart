// Frames after the first (docs/m2-dart-ui.md §8): an AnimationController
// repeating over one second, its value lerping the view from red to blue.
// Every frame is asked for by the ticker through ScheduleFrame.
import 'package:flutter/widgets.dart';

void main() => runApp(const Ticker());

class Ticker extends StatefulWidget {
  const Ticker({super.key});

  @override
  State<Ticker> createState() => _TickerState();
}

class _TickerState extends State<Ticker> with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 1),
  )..repeat();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: _controller,
      builder: (context, _) => ColoredBox(
        color: Color.lerp(
          const Color(0xFFFF0000),
          const Color(0xFF0000FF),
          _controller.value,
        )!,
      ),
    );
  }
}
