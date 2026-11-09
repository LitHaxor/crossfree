#include <metal_stdlib>
using namespace metal;

struct RuntimeConstants
{
    float4 pos_fixup;
};

static inline float4 d3d9_const(constant float4 *c, int i)
{
    return (i >= 0 && i < 224) ? c[i] : float4(0.0);
}

static inline float4 d3d9_nrm(float4 v)
{
    float len2 = dot(v.xyz, v.xyz);
    return len2 == 0.0 ? v : v * rsqrt(len2);
}

static inline float4 d3d9_lit(float4 v)
{
    float power = clamp(v.w, -128.0, 128.0);
    float specular = (v.x > 0.0 && v.y > 0.0) ? pow(v.y, power) : 0.0;
    return float4(1.0, max(v.x, 0.0), specular, 1.0);
}

static inline float4 d3d9_expp(float4 v)
{
    return float4(exp2(floor(v.x)), v.x - floor(v.x), exp2(v.x), 1.0);
}

static inline float4 d3d9_pow(float4 a, float4 b)
{
    return select(pow(abs(a), b), float4(1.0), b == float4(0.0));
}

struct ShaderInput
{
    float4 in_texcoord0 [[user(texcoord0), centroid_perspective]];
    float4 in_color0 [[user(color0)]];
};

struct ShaderOutput
{
    float4 out_color0 [[color(0)]];
};

fragment ShaderOutput shader_main(
    ShaderInput shader_in [[stage_in]],
    constant float4 *c [[buffer(0)]],
    texture2d<float> t0_texture [[texture(0)]],
    sampler s0 [[sampler(0)]],
    texturecube<float> t1_texture [[texture(1)]],
    sampler s1 [[sampler(1)]])
{
    float4 r0 = float4(0.0);
    float4 r1 = float4(0.0);
    float4 r2 = float4(0.0);
    float4 o0 = float4(0.0);
    int4 a0 = int4(0);
    int aL = 0;
    bool4 p0 = bool4(false);
    r0.xyzw = (t0_texture.sample(s0, (shader_in.in_texcoord0).xy)).xyzw;
    r1.xyzw = (t0_texture.sample(s0, ((shader_in.in_texcoord0).xy / (shader_in.in_texcoord0).w))).xyzw;
    r2.xyzw = (t1_texture.sample(s1, (shader_in.in_texcoord0).xyz, bias((shader_in.in_texcoord0).w))).xyzw;
    r0.xyzw = ((r1 * r2 + r0)).xyzw;
    r0.w = (float4(dot((r1).xy, (r2).xy) + (c[0].xxxx).x)).w;
    o0.xyzw = ((r0 * shader_in.in_color0)).xyzw;
    ShaderOutput shader_out;
    shader_out.out_color0 = o0;
    return shader_out;
}
