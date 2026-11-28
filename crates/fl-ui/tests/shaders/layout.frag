#version 460 core
#include <flutter/runtime_effect.glsl>

// Uniforms whose std140 layout is not their Dart float order: a float, a
// vec3 that pads to four, an array of vec3s, a mat3 whose columns pad, and
// two samplers between them.
uniform float uScale;
uniform vec3 uTint;
uniform sampler2D uFirst;
uniform vec3 uPoints[3];
uniform mat3 uMatrix;
uniform sampler2D uSecond;

out vec4 fragColor;

void main() {
  vec2 p = FlutterFragCoord().xy * uScale;
  vec3 c = uTint + uPoints[0] + uPoints[1] + uPoints[2];
  c = uMatrix * c;
  fragColor = vec4(c, 1.0) * texture(uFirst, p) * texture(uSecond, p);
}
