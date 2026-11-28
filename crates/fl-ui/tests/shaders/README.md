# Runtime stage fixtures

Each `.iplr` is impellerc's output for the `.frag` beside it, compiled with
the stages `flutter build` asks for on Linux:

```sh
I=.flutter/bin/cache/artifacts/engine/linux-x64
$I/impellerc --sksl --runtime-stage-gles --runtime-stage-gles3 \
  --runtime-stage-vulkan --iplr --sl=<name>.iplr --spirv=/tmp/<name>.spirv \
  --input=<name>.frag --input-type=frag --include=. --include=$I/shader_lib
```

- `time.frag`: a vec2 and a float, the usual background shader.
- `layout.frag`: uniforms whose std140 layout pads (a vec3, a vec3 array, a
  mat3) and two samplers between them.
- `ink_sparkle.frag`, `stretch_effect.frag`: Flutter's Material shaders,
  copied from the pinned SDK (`packages/flutter/lib/src/material/shaders/`),
  which every app bundle carries.
