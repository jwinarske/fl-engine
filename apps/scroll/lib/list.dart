// A list that says where it is: `offset` each time it moves, and
// `settled` each time a scroll ends, in logical pixels. In list.dart so
// test/scroll_test.dart logs the same lines under flutter_tester.
import 'package:flutter/widgets.dart';

/// A hundred rows, 50 high, of two colors.
class ScrollLog extends StatefulWidget {
  const ScrollLog({super.key, required this.log});

  final void Function(String line) log;

  @override
  State<ScrollLog> createState() => _ScrollLogState();
}

class _ScrollLogState extends State<ScrollLog> {
  final ScrollController _controller = ScrollController();

  @override
  void initState() {
    super.initState();
    _controller.addListener(() => widget.log('offset ${_controller.offset.toStringAsFixed(1)}'));
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  bool _ended(ScrollEndNotification n) {
    widget.log('settled ${n.metrics.pixels.toStringAsFixed(1)}');
    return false;
  }

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: NotificationListener<ScrollEndNotification>(
        onNotification: _ended,
        child: ListView.builder(
          controller: _controller,
          itemExtent: 50,
          itemCount: 100,
          itemBuilder: (BuildContext context, int i) =>
              ColoredBox(color: i.isEven ? const Color(0xFF2196F3) : const Color(0xFFBBDEFB)),
        ),
      ),
    );
  }
}
