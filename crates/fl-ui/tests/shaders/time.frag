#version 460 core
#include <flutter/runtime_effect.glsl>

// A background shader's usual uniforms: the size it fills and a time.
uniform vec2 uSize;
uniform float uTime;

out vec4 fragColor;

void main() {
  vec2 uv = FlutterFragCoord().xy / uSize;
  fragColor = vec4(uv, 0.5 + 0.5 * sin(uTime), 1.0);
}
