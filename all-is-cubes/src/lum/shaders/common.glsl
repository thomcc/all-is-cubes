// Copyright 2020-2021 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

// TODO: Move these debug options into being set from host code
// #define DEBUG_TEXTURE_EDGE

uniform highp mat4 projection_matrix;
uniform highp mat4 view_matrix;
uniform highp vec3 view_position;

uniform lowp sampler3D block_texture;

uniform lowp sampler3D light_texture;
uniform highp ivec3 light_offset;

// Fog equation blending: 0 is realistic fog and 1 is distant more abrupt fog.
// TODO: Replace this uniform with a compiled-in flag since it doesn't need to be continuously changing.
uniform lowp float fog_mode_blend;

// How far out should be fully fogged?
uniform highp float fog_distance;

// What color should fog fade into?
uniform mediump vec3 fog_color;


// Given integer cube coordinates, fetch and unpack a light_texture RGB value.
// The alpha component is either 0 or 1 indicating whether this is a "valid" 
// light value; one which is neither inside an opaque block nor in empty unlit
// air.
lowp vec4 light_texture_fetch(mediump vec3 p) {
  ivec3 lookup_position = ivec3(floor(p));
  lookup_position += light_offset;
  // Implement wrapping (not automatic since we're using texelFetch).
  // Wrapping is used to handle sky light and in the future will be used for
  // circular buffering of the local light in an unbounded world.
  ivec3 size = textureSize(light_texture, 0);
  lookup_position = (lookup_position % size + size) % size;

  lowp vec4 texel = texelFetch(light_texture, lookup_position, 0);
  lowp vec3 packed_light = texel.rgb;
  lowp vec3 unpacked_light = pow(vec3(2.0), (packed_light - 128.0 / 255.0) * (255.0 / 16.0));

  // See all_is_cubes::space::LightStatus for the value this is interpreting.
  bool valid = texel.a >= 0.5;

  return vec4(unpacked_light, float(valid));
}
