// Compiles src/native_add.c into libnative_add.so, the code asset
// lib/src/native_add.dart's @Native declarations are bound to. With the
// host's `cc`: the test lane builds for the machine it runs on, and one
// file needs no toolchain package.
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;
    if (input.config.code.targetOS != OS.linux) {
      throw UnsupportedError('native_add builds for Linux only');
    }
    final Uri source = input.packageRoot.resolve('src/native_add.c');
    final Uri library = input.outputDirectory.resolve('libnative_add.so');
    final ProcessResult r = await Process.run('cc', <String>[
      '-shared',
      '-fPIC',
      '-O2',
      '-o',
      library.toFilePath(),
      source.toFilePath(),
    ]);
    if (r.exitCode != 0) {
      throw Exception('cc failed: ${r.stderr}');
    }
    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: 'src/native_add.dart',
        linkMode: DynamicLoadingBundled(),
        file: library,
      ),
    );
    output.dependencies.add(source);
  });
}
