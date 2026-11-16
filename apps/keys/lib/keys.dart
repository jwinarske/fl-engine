// Every key event the framework sees, a line each: `HardwareKeyboard`'s
// `KeyEvent`s and the legacy `RawKeyboard`'s `RawKeyEvent`s, with the keys
// and locks the framework holds down after each. In keys.dart so
// test/keys_test.dart logs the same lines under flutter_tester.
//
// ignore_for_file: deprecated_member_use
import 'package:flutter/services.dart';

String _hex(int v) => '0x${v.toRadixString(16)}';

/// A character as a line can show it: quoted, a control character by its
/// code point; `-` for none.
String _character(String? c) {
  if (c == null) {
    return '-';
  }
  final String shown = c.runes
      .map((int r) => r < 0x20 || r == 0x7f ? '\\u{${r.toRadixString(16)}}' : String.fromCharCode(r))
      .join();
  return '"$shown"';
}

String _sorted(Iterable<int> ids) => (ids.toList()..sort()).map(_hex).join(',');

class KeyLog {
  KeyLog(this.emit);

  /// Given each line as its event arrives.
  final void Function(String line) emit;

  void attach() {
    HardwareKeyboard.instance.addHandler(_key);
    RawKeyboard.instance.addListener(_raw);
  }

  void detach() {
    HardwareKeyboard.instance.removeHandler(_key);
    RawKeyboard.instance.removeListener(_raw);
  }

  /// Handles key A, so the engine is told of a key the framework took and
  /// of ones it did not. F1 going down asks the engine which keys it holds
  /// down (`getKeyboardState`).
  bool _key(KeyEvent e) {
    final String kind = switch (e) {
      KeyDownEvent() => 'down',
      KeyUpEvent() => 'up',
      KeyRepeatEvent() => 'repeat',
      _ => '?',
    };
    final HardwareKeyboard keyboard = HardwareKeyboard.instance;
    emit(
      'key $kind physical=${_hex(e.physicalKey.usbHidUsage)} logical=${_hex(e.logicalKey.keyId)} '
      'character=${_character(e.character)} synthesized=${e.synthesized} '
      'pressed=[${_sorted(keyboard.logicalKeysPressed.map((LogicalKeyboardKey k) => k.keyId))}] '
      'locks=[${(keyboard.lockModesEnabled.map((KeyboardLockMode m) => m.name).toList()..sort()).join(',')}]',
    );
    if (e is KeyDownEvent && e.logicalKey == LogicalKeyboardKey.f1) {
      _state();
    }
    return e.logicalKey == LogicalKeyboardKey.keyA;
  }

  Future<void> _state() async {
    final Map<int, int>? state = await SystemChannels.keyboard.invokeMapMethod<int, int>('getKeyboardState');
    final List<int> physical = (state ?? <int, int>{}).keys.toList()..sort();
    emit('state {${physical.map((int p) => '${_hex(p)}: ${_hex(state![p]!)}').join(', ')}}');
  }

  void _raw(RawKeyEvent e) {
    final String kind = e is RawKeyDownEvent ? 'down' : 'up';
    emit(
      'raw $kind physical=${_hex(e.physicalKey.usbHidUsage)} logical=${_hex(e.logicalKey.keyId)} '
      'character=${_character(e.character)} repeat=${e.repeat} '
      'shift=${e.isShiftPressed} control=${e.isControlPressed} alt=${e.isAltPressed} meta=${e.isMetaPressed}',
    );
  }
}

/// What the engine was given, for a line of its own: not what
/// flutter_tester's simulated events carry, so not compared with it.
String rawData(RawKeyEvent e) {
  final RawKeyEventData data = e.data;
  if (data is! RawKeyEventDataLinux) {
    return 'data ${data.runtimeType}';
  }
  return 'data toolkit=${data.keyHelper.runtimeType} keyCode=${_hex(data.keyCode)} scanCode=${_hex(data.scanCode)} '
      'modifiers=${_hex(data.modifiers)} unicodeScalarValues=${_hex(data.unicodeScalarValues)} '
      'specifiedLogicalKey=${data.specifiedLogicalKey == null ? '-' : _hex(data.specifiedLogicalKey!)}';
}
