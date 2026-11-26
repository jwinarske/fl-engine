// A Texture widget over a pixel-buffer texture registered through Flutter's
// desktop C API (flutter_texture_registrar.h), looked up in the process as
// a plugin's library resolves it. The copy callback is fl-dart's test
// producer (fl_dart_texture_pattern), since it runs on the raster thread.
// A moment after the first frame the producer moves on a phase and marks a
// frame available: the raster thread draws the last scene again with it.
//
// With FL_TEXTURE=dmabuf in the environment the texture is a GPU surface
// instead, a dma-buf (fl-engine's surface type 4) that fl-dart's other test
// producer (fl_dart_texture_dmabuf) draws with a Vulkan device of its own:
// the same pattern, in two buffers it hands over in turn.
import 'dart:async';
import 'dart:ffi';
import 'dart:io';

import 'package:flutter/widgets.dart';

import 'textures.dart';

typedef _GetRegistrar = Pointer<Void> Function(Pointer<Void>);
typedef _Register = Int64 Function(Pointer<Void>, Pointer<Uint8>);
typedef _RegisterDart = int Function(Pointer<Void>, Pointer<Uint8>);
typedef _Init = Bool Function();
typedef _InitDart = bool Function();
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
  final bool dmaBuf = Platform.environment['FL_TEXTURE'] == 'dmabuf';
  final next = process.lookupFunction<_Next, _NextDart>(
    dmaBuf ? 'fl_dart_texture_dmabuf_next' : 'fl_dart_texture_next',
  );
  final calloc = process.lookupFunction<_Calloc, _CallocDart>('calloc');

  // FlutterDesktopTextureInfo: the type at 0, the union from 8; 40 bytes,
  // the GPU-surface config being the larger member. Never freed: the
  // texture lives as long as the app.
  final Pointer<Uint8> info = calloc(1, 40);
  if (dmaBuf) {
    final init = process.lookupFunction<_Init, _InitDart>('fl_dart_texture_dmabuf_init');
    print('producer ${init()}');
    // A GPU surface (1): its config's struct_size (32) at 8, the surface
    // type (4, a dma-buf) at 16, callback at 24, user_data at 32.
    info.cast<Int32>().value = 1;
    (info + 8).cast<IntPtr>().value = 32;
    (info + 16).cast<Int32>().value = 4;
    (info + 24).cast<Pointer<Void>>().value = process.lookup<Void>('fl_dart_texture_dmabuf');
  } else {
    // A pixel buffer (0): its config's callback at 8, user_data at 16.
    info.cast<Int32>().value = 0;
    (info + 8).cast<Pointer<Void>>().value = process.lookup<Void>('fl_dart_texture_pattern');
  }
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
