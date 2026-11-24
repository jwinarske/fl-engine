// A Texture widget over a pixel-buffer texture registered through Flutter's
// desktop C API (flutter_texture_registrar.h), looked up in the process as
// a plugin's library resolves it. The copy callback is fl-dart's test
// producer (fl_dart_texture_pattern), since it runs on the raster thread.
// A moment after the first frame the producer moves on a phase and marks a
// frame available: the raster thread draws the last scene again with it.
import 'dart:async';
import 'dart:ffi';

import 'package:flutter/widgets.dart';

import 'textures.dart';

typedef _GetRegistrar = Pointer<Void> Function(Pointer<Void>);
typedef _Register = Int64 Function(Pointer<Void>, Pointer<Uint8>);
typedef _RegisterDart = int Function(Pointer<Void>, Pointer<Uint8>);
typedef _Next = Bool Function(Int64);
typedef _NextDart = bool Function(int);
typedef _Calloc = Pointer<Uint8> Function(IntPtr, IntPtr);
typedef _CallocDart = Pointer<Uint8> Function(int, int);

void main() {
  final DynamicLibrary process = DynamicLibrary.process();
  final getRegistrar = process.lookupFunction<_GetRegistrar, _GetRegistrar>(
    'FlutterDesktopRegistrarGetTextureRegistrar',
  );
  final register = process.lookupFunction<_Register, _RegisterDart>(
    'FlutterDesktopTextureRegistrarRegisterExternalTexture',
  );
  final next = process.lookupFunction<_Next, _NextDart>('fl_dart_texture_next');
  final calloc = process.lookupFunction<_Calloc, _CallocDart>('calloc');

  // FlutterDesktopTextureInfo: the type (0, a pixel buffer) at 0, the
  // union's pixel_buffer_config (callback, user_data) from 8; 40 bytes, the
  // GPU-surface config being the larger member. Never freed: the texture
  // lives as long as the app.
  final Pointer<Uint8> info = calloc(1, 40);
  info.cast<Int32>().value = 0;
  (info + 8).cast<Pointer<Void>>().value = process.lookup<Void>('fl_dart_texture_pattern');
  final int id = register(getRegistrar(nullptr), info);
  print('texture $id');

  runApp(
    Textures(
      view: (FilterQuality quality) => Texture(textureId: id, filterQuality: quality),
    ),
  );
  WidgetsBinding.instance.addPostFrameCallback((_) {
    Timer(const Duration(milliseconds: 300), () => print('next ${next(id)}'));
  });
}
