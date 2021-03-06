#version 450

layout(location=0) in vec3 a_position;
layout(location=1) in vec4 a_color;
layout(location=2) in vec2 a_tex_coords;

layout(location=0) out vec4 v_color;
layout(location=1) out vec2 v_tex_coords;

layout(set=1, binding=0)
uniform Uniforms {
    mat4 combined;
};

void main() {
    gl_Position = combined * vec4(a_position, 1.0);
    v_color = a_color;
    v_tex_coords = a_tex_coords;
}
