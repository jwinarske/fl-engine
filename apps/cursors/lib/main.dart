// A cell for every system mouse cursor the framework has, each asking for
// its own as the mouse enters it, to look at on a board's screen
// (docs/m4-input.md stage 6). A cell says the framework's name for the
// cursor and the cursor theme's; the one the mouse is in is printed.
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// The framework's kinds and the names the Linux embedder looks them up by
/// in a cursor theme (fl_mouse_cursor_handler.cc), as fl-input has them.
const cursors = <(SystemMouseCursor, String)>[
  (SystemMouseCursors.basic, 'default'),
  (SystemMouseCursors.click, 'pointer'),
  (SystemMouseCursors.text, 'text'),
  (SystemMouseCursors.verticalText, 'vertical-text'),
  (SystemMouseCursors.precise, 'crosshair'),
  (SystemMouseCursors.cell, 'cell'),
  (SystemMouseCursors.help, 'help'),
  (SystemMouseCursors.contextMenu, 'context-menu'),
  (SystemMouseCursors.wait, 'wait'),
  (SystemMouseCursors.progress, 'progress'),
  (SystemMouseCursors.forbidden, 'not-allowed'),
  (SystemMouseCursors.noDrop, 'no-drop'),
  (SystemMouseCursors.alias, 'alias'),
  (SystemMouseCursors.copy, 'copy'),
  (SystemMouseCursors.move, 'move'),
  (SystemMouseCursors.allScroll, 'all-scroll'),
  (SystemMouseCursors.grab, 'grab'),
  (SystemMouseCursors.grabbing, 'grabbing'),
  (SystemMouseCursors.zoomIn, 'zoom-in'),
  (SystemMouseCursors.zoomOut, 'zoom-out'),
  (SystemMouseCursors.resizeColumn, 'col-resize'),
  (SystemMouseCursors.resizeRow, 'row-resize'),
  (SystemMouseCursors.resizeLeftRight, 'ew-resize'),
  (SystemMouseCursors.resizeUpDown, 'ns-resize'),
  (SystemMouseCursors.resizeUpLeftDownRight, 'nwse-resize'),
  (SystemMouseCursors.resizeUpRightDownLeft, 'nesw-resize'),
  (SystemMouseCursors.resizeUp, 'n-resize'),
  (SystemMouseCursors.resizeDown, 's-resize'),
  (SystemMouseCursors.resizeLeft, 'w-resize'),
  (SystemMouseCursors.resizeRight, 'e-resize'),
  (SystemMouseCursors.resizeUpLeft, 'nw-resize'),
  (SystemMouseCursors.resizeUpRight, 'ne-resize'),
  (SystemMouseCursors.resizeDownLeft, 'sw-resize'),
  (SystemMouseCursors.resizeDownRight, 'se-resize'),
  (SystemMouseCursors.none, 'none'),
];

void main() => runApp(const Cursors(log: print));

class Cursors extends StatelessWidget {
  const Cursors({super.key, required this.log});

  final void Function(String) log;

  static const _columns = 5;

  @override
  Widget build(BuildContext context) {
    final rows = <Widget>[];
    for (var i = 0; i < cursors.length; i += _columns) {
      rows.add(
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (var j = i; j < i + _columns; j++)
                Expanded(
                  child: j < cursors.length
                      ? _Cell(cursors[j].$1, cursors[j].$2, j, log)
                      : const SizedBox(),
                ),
            ],
          ),
        ),
      );
    }
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: const Color(0xFF202124),
        child: Column(children: rows),
      ),
    );
  }
}

class _Cell extends StatelessWidget {
  const _Cell(this.cursor, this.theme, this.index, this.log);

  final SystemMouseCursor cursor;
  final String theme;
  final int index;
  final void Function(String) log;

  @override
  Widget build(BuildContext context) {
    // Two shades, as a board's squares, so a cell's edge is where the
    // cursor changes.
    final dark = (index ~/ Cursors._columns + index % Cursors._columns).isEven;
    return MouseRegion(
      cursor: cursor,
      onEnter: (_) => log('cursor ${cursor.kind} $theme'),
      child: ColoredBox(
        color: Color(dark ? 0xFF2D3748 : 0xFF4A5568),
        child: Center(
          child: Text(
            '${cursor.kind}\n$theme',
            textAlign: TextAlign.center,
            style: const TextStyle(color: Color(0xFFFFFFFF), fontSize: 14),
          ),
        ),
      ),
    );
  }
}
