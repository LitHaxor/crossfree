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
    float4 out_texcoord0 [[user(texcoord0)]];
};

vertex ShaderOutput shader_main(
    ShaderInput shader_in [[stage_in]],
    constant float4 *c [[buffer(0)]],
    constant int4 *ci [[buffer(1)]],
    constant uint *cb [[buffer(2)]],
    constant RuntimeConstants &runtime [[buffer(3)]],
    texture2d<float> t0_texture [[texture(0)]],
    sampler s0 [[sampler(0)]])
{
    float4 r0 = float4(0.0);
    float4 r1 = float4(0.0);
    float4 o0 = float4(0.0);
    float4 o1 = float4(0.0);
    int4 a0 = int4(0);
    int aL = 0;
    bool4 p0 = bool4(false);
    a0.x = int(round((shader_in.v1.xxxx).x));
    r0.xyzw = (d3d9_const(c, 10 + a0.x)).xyzw;
    {
        int4 loop_control0 = int4(3, 0, 1, 0);
        int saved_aL0 = aL;
        aL = loop_control0.y;
        for (int loop0 = 0; loop0 < min(loop_control0.x, 255); ++loop0, aL += loop_control0.z)
        {
            r0.xyzw = ((r0 + d3d9_const(c, 20 + aL))).xyzw;
            if (((r0.xxxx).x > (c[1].xxxx).x)) break;
        }
        aL = saved_aL0;
    }
    for (int loop0 = 0; loop0 < min((ci[1]).x, 255); ++loop0)
    {
        if (true)
        {
            r0.xyzw = ((r0 * c[2])).xyzw;
        }
        else
        {
            r0.xyzw = ((r0 + c[3])).xyzw;
        }
    }
    p0.xyzw = ((shader_in.v1 < c[4])).xyzw;
    {
        auto value = c[5];
        if (p0.x) r0.x = value.x;
        if (p0.y) r0.y = value.y;
        if (p0.z) r0.z = value.z;
        if (p0.w) r0.w = value.w;
    }
    if ((cb[1] != 0u))
    {
        r1.xy = (float4(cos((r0.wwww).x), sin((r0.wwww).x), 0.0, 0.0)).xy;
        r0.xyzw = ((r0 + r1)).xyzw;
    }
    r1.xyzw = (t0_texture.sample(s0, (shader_in.v1).xy, level((shader_in.v1).w))).xyzw;
    o0.x = (float4(dot(shader_in.v0, c[40]))).x;
    o0.y = (float4(dot(shader_in.v0, c[41]))).y;
    o0.z = (float4(dot(shader_in.v0, c[42]))).z;
    o0.w = (float4(dot(shader_in.v0, c[43]))).w;
    o1.xyzw = ((r0 + r1)).xyzw;
    ShaderOutput shader_out;
    shader_out.position = o0;
    shader_out.position.xy = shader_out.position.xy * runtime.pos_fixup.xy + runtime.pos_fixup.zw * shader_out.position.w;
    shader_out.out_texcoord0 = o1;
    return shader_out;
}
