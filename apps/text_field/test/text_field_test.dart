// The references: the stock engine's flutter_tester, with Impeller, given
// the edits test/*.txt give fl-dart as a script of input events.
//
//     flutter test --enable-impeller --update-goldens
//
// The frames after the text is typed, after a word is selected and after
// it is pasted are the goldens fl-engine's are held against. The lines the
// field logs, an editing value each time it changes, are held in
// test/*_events.txt (docs/m4-input.md §9, D5).
//
// flutter_tester gives its input to the framework's binding, above the
// engine: where the engine's text input handler tells the framework what a
// key typed, the test tells it the same editing state on the same channel
// (`TestTextInput.updateEditingValue`), the states the Linux embedder
// sends for those keys. Keys the framework handles itself are sent as keys.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:text_field/field.dart';

Future<void> _load(String family, String file) async {
  final String root = Platform.environment['FLUTTER_ROOT'] ?? '';
  final bytes = File('$root/bin/cache/artifacts/material_fonts/$file').readAsBytesSync();
  await (FontLoader(family)..addFont(Future<ByteData>.value(ByteData.sublistView(bytes)))).load();
}

/// What a scripted run does to the field, under flutter_tester.
class Editor {
  Editor(this.tester);

  final WidgetTester tester;

  /// The editing state the engine's handler sends once a key has typed:
  /// the text, the caret after what was typed, and what is being composed.
  Future<void> state(String text, {int? caret, TextRange composing = TextRange.empty}) async {
    tester.testTextInput.updateEditingValue(
      TextEditingValue(
        text: text,
        selection: TextSelection.collapsed(offset: caret ?? text.length),
        composing: composing,
      ),
    );
    await tester.pumpAndSettle();
  }

  /// A key the framework handles: tapped, with the keys of `held` down.
  Future<void> key(LogicalKeyboardKey key, {LogicalKeyboardKey? held}) async {
    if (held != null) {
      await tester.sendKeyDownEvent(held, platform: 'linux');
    }
    await tester.sendKeyEvent(key, platform: 'linux');
    if (held != null) {
      await tester.sendKeyUpEvent(held, platform: 'linux');
    }
    await tester.pumpAndSettle();
  }
}

/// Runs `edit` on the field at fl-engine's app-lane size and holds its log
/// against `test/<name>_events.txt`.
void edits(String name, Future<void> Function(WidgetTester tester, Editor e) edit, {String? seed}) {
  testWidgets(name, (WidgetTester tester) async {
    await tester.runAsync(() async {
      await _load('Roboto', 'Roboto-Regular.ttf');
      await _load('MaterialIcons', 'MaterialIcons-Regular.otf');
    });
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    EditableText.debugDeterministicCursor = true;

    // The engine's clipboard: text, kept in the process.
    String? clipboard;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (MethodCall call) async {
      switch (call.method) {
        case 'Clipboard.setData':
          clipboard = (call.arguments as Map<Object?, Object?>)['text'] as String?;
          return null;
        case 'Clipboard.getData':
          return clipboard == null ? null : <String, Object?>{'text': clipboard};
        case 'Clipboard.hasStrings':
          return <String, Object?>{'value': clipboard?.isNotEmpty ?? false};
      }
      return null;
    });

    final List<String> lines = <String>[];
    final TextEditingController controller = TextEditingController();
    listenForSeed(controller);
    await tester.pumpWidget(TextFieldApp(log: lines.add, controller: controller));
    await tester.pumpAndSettle();
    if (seed != null) {
      // As `fl-dart run-app --message fl/seed=...` sends it: the field has
      // the keyboard by then, and tells the engine its new text.
      await tester.binding.defaultBinaryMessenger.handlePlatformMessage(
        'fl/seed',
        ByteData.sublistView(const Utf8Encoder().convert(seed)),
        (ByteData? _) {},
      );
      await tester.pumpAndSettle();
    }

    await edit(tester, Editor(tester));

    final String out = '${lines.join('\n')}\n';
    final File expected = File('test/${name}_events.txt');
    if (autoUpdateGoldenFiles) {
      expected.writeAsStringSync(out);
    } else {
      expect(out, expected.readAsStringSync());
    }
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null);
    EditableText.debugDeterministicCursor = false;
    debugDefaultTargetPlatformOverride = null;
  });
}

void main() {
  edits('edit', (WidgetTester tester, Editor e) async {
    // test/typed.txt: letters, and an e under a dead acute. The dead key
    // shows as the text being composed, which is taken away as the
    // composed character is put in.
    await e.state('c');
    await e.state('ca');
    await e.state('caf');
    await e.state('caf´', composing: const TextRange(start: 3, end: 4));
    await e.state('caf');
    await e.state('café');
    await e.state('café ');
    await e.state('café b');
    await e.state('café ba');
    await e.state('café bar');
    await expectLater(find.byType(TextFieldApp), matchesGoldenFile('goldens/text_field_typed.png'));

    // test/selected.txt: the mouse dragged across the first word.
    final TestGesture mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: const Offset(45, 68));
    await mouse.down(const Offset(45, 68));
    await tester.pumpAndSettle();
    await mouse.moveTo(const Offset(98, 68));
    await tester.pumpAndSettle();
    await mouse.up();
    await tester.pumpAndSettle();
    await expectLater(find.byType(TextFieldApp), matchesGoldenFile('goldens/text_field_selected.png'));

    // test/pasted.txt: copied, pasted at the end, a character deleted.
    await e.key(LogicalKeyboardKey.keyC, held: LogicalKeyboardKey.controlLeft);
    await e.key(LogicalKeyboardKey.end);
    await e.key(LogicalKeyboardKey.keyV, held: LogicalKeyboardKey.controlLeft);
    await e.key(LogicalKeyboardKey.backspace);
    await expectLater(find.byType(TextFieldApp), matchesGoldenFile('goldens/text_field_pasted.png'));

    // test/submitted.txt: Return, which the engine's handler makes the
    // field's action.
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
  });

  // test/emoji.txt: a field that starts with a character of two UTF-16
  // code units.
  edits('emoji', seed: '😄 ', (WidgetTester tester, Editor e) async {
    await e.state('😄 a');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft, platform: 'linux');
    await e.key(LogicalKeyboardKey.arrowLeft);
    await e.key(LogicalKeyboardKey.arrowLeft);
    await e.key(LogicalKeyboardKey.arrowLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft, platform: 'linux');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft, platform: 'linux');
    await e.key(LogicalKeyboardKey.keyX);
    await e.key(LogicalKeyboardKey.keyV);
    await e.key(LogicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft, platform: 'linux');
    await e.key(LogicalKeyboardKey.home);
    await e.key(LogicalKeyboardKey.delete);
    await e.state('z a😄 a', caret: 1);
  });
}
