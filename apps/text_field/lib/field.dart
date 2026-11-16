// A text field that says what is in it, a line each time that changes:
// its text, its selection and the range being composed, in UTF-16 code
// units as the framework counts them. In field.dart so
// test/text_field_test.dart logs the same lines under flutter_tester.
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

String _range(TextRange r) => r.isValid ? '${r.start}..${r.end}' : '-';

/// One editing value as a line.
String describe(TextEditingValue v) {
  final TextSelection s = v.selection;
  final String selection = s.isValid ? '${s.baseOffset}..${s.extentOffset}' : '-';
  return 'value text=${jsonEncode(v.text)} selection=$selection composing=${_range(v.composing)}';
}

/// Puts the text of a message on `fl/seed` in the field, the caret after
/// it.
void listenForSeed(TextEditingController controller) {
  ServicesBinding.instance.defaultBinaryMessenger.setMessageHandler('fl/seed', (ByteData? data) async {
    final String text = data == null
        ? ''
        : utf8.decode(data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes));
    controller.value = TextEditingValue(
      text: text,
      selection: TextSelection.collapsed(offset: text.length),
    );
    return null;
  });
}

class TextFieldApp extends StatefulWidget {
  const TextFieldApp({super.key, required this.log, required this.controller});

  /// Given each line as its change arrives.
  final void Function(String line) log;
  final TextEditingController controller;

  @override
  State<TextFieldApp> createState() => _TextFieldAppState();
}

class _TextFieldAppState extends State<TextFieldApp> {
  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_changed);
  }

  @override
  void dispose() {
    widget.controller.removeListener(_changed);
    super.dispose();
  }

  void _changed() => widget.log(describe(widget.controller.value));

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: Padding(
          padding: const EdgeInsets.all(40),
          child: TextField(
            autofocus: true,
            controller: widget.controller,
            style: const TextStyle(fontSize: 32),
            onSubmitted: (String text) => widget.log('submitted ${jsonEncode(text)}'),
          ),
        ),
      ),
    );
  }
}
