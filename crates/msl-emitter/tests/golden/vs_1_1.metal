#include <metal_stdlib>
using namespace metal;

struct RuntimeConstants
{
    float4 pos_fixup;
};

static inline float4 d3d9_const(constant float4 *c, int i)
{
    return (i >= 0 && i < 256) ? c[i] : float4(0.0);
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
    float4 v0 [[attribute(0)]];
    float4 v1 [[attribute(1)]];
};

struct ShaderOutput
{
    float4 position [[position]];
    float4 out_color0 [[user(color0)]];
    float4 out_texcoord0 [[user(texcoord0)]];
    float4 out_fog [[user(fog)]];
    float4 out_texcoord1 [[user(texcoord1)]];
    float4 out_color1 [[user(color1)]];
};

vertex ShaderOutput shader_main(
    ShaderInput shader_in [[stage_in]],
    constant float4 *c [[buffer(0)]],
    constant RuntimeConstants &runtime [[buffer(3)]])
{
    float4 r0 = float4(0.0);
    float4 r1 = float4(0.0);
    float4 r2 = float4(0.0);
    float4 o0 = float4(0.0);
    float4 o1 = float4(0.0);
    float4 o2 = float4(0.0);
    float4 o3 = float4(0.0);
    float4 o4 = float4(0.0);
    float4 o5 = float4(0.0);
    int4 a0 = int4(0);
    int aL = 0;
    bool4 p0 = bool4(false);
    o0.x = (float4(dot(shader_in.v0, c[0]))).x;
    o0.y = (float4(dot(shader_in.v0, c[1]))).y;
    o0.z = (float4(dot(shader_in.v0, c[2]))).z;
    o0.w = (float4(dot(shader_in.v0, c[3]))).w;
    a0.x = int(floor((c[4].xxxx).x));
    o1.xyzw = (d3d9_const(c, 5 + a0.x)).xyzw;
    o2.xyzw = ((shader_in.v1 * float4(5e-1, 1.0, 2.0, -1.0))).xyzw;
    o3.x = (float4(5e-1, 1.0, 2.0, -1.0).wwww).x;
    r0.x = ((1.0 / c[9].xxxx)).x;
    r0.y = (rsqrt(abs(c[9].yyyy))).y;
    r1.xyzw = (d3d9_expp(c[9].zzzz)).xyzw;
    r2.xyzw = (d3d9_lit(c[10])).xyzw;
    o4.xyzw = ((r0 * r1)).xyzw;
    o5.xyzw = (r2).xyzw;
    ShaderOutput shader_out;
    shader_out.position = o0;
    shader_out.position.xy = shader_out.position.xy * runtime.pos_fixup.xy + runtime.pos_fixup.zw * shader_out.position.w;
    shader_out.out_color0 = saturate(o1);
    shader_out.out_texcoord0 = o2;
    shader_out.out_fog = o3;
    shader_out.out_texcoord1 = o4;
    shader_out.out_color1 = saturate(o5);
    return shader_out;
}
