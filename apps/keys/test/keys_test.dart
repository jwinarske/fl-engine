// The reference lines: the stock framework under flutter_tester, sent the
// keys test/typing.txt sends fl-dart, logs what it sees of them
// (docs/m4-input.md §9, D5).
//
//     flutter test --update-goldens
//
// flutter_tester gives its events to the framework's binding, above the
// engine, so what is held here is what the framework makes of a key, not
// the message it arrived in: the `data` lines are fl-dart's alone.
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:keys/keys.dart';

/// Sends keys as test/typing.txt and test/french.txt do, and holds the
/// log against `test/<name>_events.txt`.
void keys(String name, Future<void> Function(Typist t) send) {
  testWidgets(name, (WidgetTester tester) async {
    final List<String> lines = <String>[];
    // The engine's answer to `getKeyboardState`: the keys sent as down.
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.keyboard, (MethodCall call) async {
      final HardwareKeyboard keyboard = HardwareKeyboard.instance;
      return <int, int>{
        for (final PhysicalKeyboardKey key in keyboard.physicalKeysPressed)
          key.usbHidUsage: keyboard.lookUpLayout(key)!.keyId,
      };
    });
    await tester.pumpWidget(const ColoredBox(color: Color(0xFF2196F3)));
    final KeyLog log = KeyLog(lines.add)..attach();
    await send(Typist(tester));
    log.detach();
    final String out = '${lines.join('\n')}\n';
    final File expected = File('test/${name}_events.txt');
    if (autoUpdateGoldenFiles) {
      expected.writeAsStringSync(out);
    } else {
      expect(out, expected.readAsStringSync());
    }
  });
}

/// A key is its place on the keyboard, what the layout makes it, and what
/// it types, as a Linux keyboard's events say.
class Typist {
  Typist(this.tester);

  final WidgetTester tester;

  Future<void> down(LogicalKeyboardKey key, PhysicalKeyboardKey physical, [String? character]) =>
      tester.sendKeyDownEvent(key, platform: 'linux', physicalKey: physical, character: character);

  Future<void> repeat(LogicalKeyboardKey key, PhysicalKeyboardKey physical, [String? character]) =>
      tester.sendKeyRepeatEvent(key, platform: 'linux', physicalKey: physical, character: character);

  Future<void> up(LogicalKeyboardKey key, PhysicalKeyboardKey physical) =>
      tester.sendKeyUpEvent(key, platform: 'linux', physicalKey: physical);

  Future<void> tap(LogicalKeyboardKey key, PhysicalKeyboardKey physical, [String? character]) async {
    await down(key, physical, character);
    await up(key, physical);
  }
}

void main() {
  keys('typing', (Typist t) async {
    // A letter.
    await t.tap(LogicalKeyboardKey.keyA, PhysicalKeyboardKey.keyA, 'a');
    // Shift held over a letter.
    await t.down(LogicalKeyboardKey.shiftLeft, PhysicalKeyboardKey.shiftLeft);
    await t.tap(LogicalKeyboardKey.keyA, PhysicalKeyboardKey.keyA, 'A');
    await t.up(LogicalKeyboardKey.shiftLeft, PhysicalKeyboardKey.shiftLeft);
    // Caps Lock on, a letter, off, a letter.
    await t.tap(LogicalKeyboardKey.capsLock, PhysicalKeyboardKey.capsLock);
    await t.tap(LogicalKeyboardKey.keyB, PhysicalKeyboardKey.keyB, 'B');
    await t.tap(LogicalKeyboardKey.capsLock, PhysicalKeyboardKey.capsLock);
    await t.tap(LogicalKeyboardKey.keyB, PhysicalKeyboardKey.keyB, 'b');
    // A key held.
    await t.down(LogicalKeyboardKey.keyC, PhysicalKeyboardKey.keyC, 'c');
    await t.repeat(LogicalKeyboardKey.keyC, PhysicalKeyboardKey.keyC, 'c');
    await t.repeat(LogicalKeyboardKey.keyC, PhysicalKeyboardKey.keyC, 'c');
    await t.up(LogicalKeyboardKey.keyC, PhysicalKeyboardKey.keyC);
    // The number pad with Num Lock on, and off.
    await t.tap(LogicalKeyboardKey.numLock, PhysicalKeyboardKey.numLock);
    await t.tap(LogicalKeyboardKey.numpad1, PhysicalKeyboardKey.numpad1, '1');
    await t.tap(LogicalKeyboardKey.numLock, PhysicalKeyboardKey.numLock);
    await t.tap(LogicalKeyboardKey.numpad1, PhysicalKeyboardKey.numpad1);
    // A shortcut's keys.
    await t.down(LogicalKeyboardKey.controlLeft, PhysicalKeyboardKey.controlLeft);
    await t.tap(LogicalKeyboardKey.keyV, PhysicalKeyboardKey.keyV, 'v');
    await t.up(LogicalKeyboardKey.controlLeft, PhysicalKeyboardKey.controlLeft);
    // Keys that type no character.
    await t.tap(LogicalKeyboardKey.enter, PhysicalKeyboardKey.enter, '\r');
    await t.tap(LogicalKeyboardKey.arrowLeft, PhysicalKeyboardKey.arrowLeft);
    await t.tap(LogicalKeyboardKey.escape, PhysicalKeyboardKey.escape, '\x1b');
    // Which keys the engine holds down, asked while two are.
    await t.down(LogicalKeyboardKey.shiftRight, PhysicalKeyboardKey.shiftRight);
    await t.down(LogicalKeyboardKey.f1, PhysicalKeyboardKey.f1);
    await t.tester.pump();
    await t.up(LogicalKeyboardKey.f1, PhysicalKeyboardKey.f1);
    await t.up(LogicalKeyboardKey.shiftRight, PhysicalKeyboardKey.shiftRight);
  });

  keys('french', (Typist t) async {
    // Key A types q, and is logical key Q.
    await t.tap(LogicalKeyboardKey.keyQ, PhysicalKeyboardKey.keyA, 'q');
    // Digit 1 types &, and 1 with Shift: logical key 1 both ways.
    await t.tap(LogicalKeyboardKey.digit1, PhysicalKeyboardKey.digit1, '&');
    await t.down(LogicalKeyboardKey.shiftLeft, PhysicalKeyboardKey.shiftLeft);
    await t.tap(LogicalKeyboardKey.digit1, PhysicalKeyboardKey.digit1, '1');
    await t.up(LogicalKeyboardKey.shiftLeft, PhysicalKeyboardKey.shiftLeft);
    // The key right of L types m; the key M is on a US keyboard types a comma.
    await t.tap(LogicalKeyboardKey.keyM, PhysicalKeyboardKey.semicolon, 'm');
    await t.tap(LogicalKeyboardKey.comma, PhysicalKeyboardKey.keyM, ',');
  });
}
