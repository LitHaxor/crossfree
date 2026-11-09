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
    float4 in_texcoord0 [[user(texcoord0)]];
    float4 in_color1 [[user(color1)]];
};

struct ShaderOutput
{
    float4 out_color0 [[color(0)]];
    float depth [[depth(any)]];
};

fragment ShaderOutput shader_main(
    ShaderInput shader_in [[stage_in]],
    float4 frag_position [[position]],
    bool front_facing [[front_facing]],
    constant float4 *c [[buffer(0)]],
    constant int4 *ci [[buffer(1)]],
    texture2d<float> t0_texture [[texture(0)]],
    sampler s0 [[sampler(0)]],
    texture3d<float> t1_texture [[texture(1)]],
    sampler s1 [[sampler(1)]])
{
    float4 r0 = float4(0.0);
    float4 r1 = float4(0.0);
    float4 o0 = float4(0.0);
    float4 o1 = float4(0.0);
    int4 a0 = int4(0);
    int aL = 0;
    bool4 p0 = bool4(false);
    float4 vpos = float4(floor(frag_position.xy), frag_position.zw);
    float4 vface = float4(front_facing ? 1.0 : -1.0);
    r0.xyzw = (t0_texture.sample(s0, (shader_in.in_texcoord0).xy).yzwx).xyzw;
    r1.xyzw = (t1_texture.sample(s1, (shader_in.in_texcoord0).xyz, gradient3d((shader_in.in_color1).xyz, (shader_in.in_color1).xyz))).xyzw;
    p0.xyzw = ((shader_in.in_color1 >= c[0])).xyzw;
    if (p0.x)
    {
        r0.xyzw = ((r0 + vpos.xyxy)).xyzw;
    }
    for (int loop0 = 0; loop0 < min((int4(4, 0, 0, 0)).x, 255); ++loop0)
    {
        if (!p0.y) break;
        r0.xyzw = ((r0 * c[1])).xyzw;
    }
    {
        auto value = r1.zzzz;
        if (!p0.z) r0.z = value.z;
    }
    o0.xyzw = (select(r1, r0, vface.xxxx >= float4(0.0))).xyzw;
    o1.x = (r0.wwww).x;
    ShaderOutput shader_out;
    shader_out.out_color0 = o0;
    shader_out.depth = o1.x;
    return shader_out;
}
