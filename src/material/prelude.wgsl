struct Frame {
    capture : vec4<f32>,
    body : vec4<f32>,
    shape : vec4<f32>,
    notch : vec4<f32>,
    env : vec4<f32>,
    tint : vec4<f32>,
    tex : vec4<f32>,
    count : vec4<u32>,
    ctl_rect : array<vec4<f32>, 16>,
    ctl_shape : array<vec4<f32>, 16>,
    ctl_spring : array<vec4<f32>, 16>,
    params : array<vec4<f32>, 8>,
};
@group(0) @binding(0) var<uniform> frame : Frame;
@group(0) @binding(1) var backdrop : texture_2d<f32>;
@group(0) @binding(2) var backdrop_sampler : sampler;

struct MaterialIn { px : vec2<f32>, uv : vec2<f32> };

fn param(i : u32) -> f32 {
    return frame.params[i / 4u][i % 4u];
}

fn rr_sdf(p : vec2<f32>, b : vec2<f32>, r : f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

fn notch_sdf(p : vec2<f32>, b : vec2<f32>, r : f32, flare : f32, fh : f32) -> f32 {
    let t = clamp((p.y + b.y) / max(fh, 1.0), 0.0, 1.0);
    let wing = flare * pow(1.0 - t, 2.4);
    let hx = b.x + wing;
    if (p.y > b.y - r) {
        let qx = max(abs(p.x) - (b.x - r), 0.0);
        let qy = abs(p.y) - (b.y - r);
        return length(vec2<f32>(qx, max(qy, 0.0))) - r;
    }
    return max(abs(p.x) - hx, abs(p.y) - b.y);
}

fn body_sdf(p : vec2<f32>) -> f32 {
    // 沿用原玻璃轮廓的内缩，给窗口区域内的羽化和描边留出空间。
    let inset = 3.0;
    var half_size = max(frame.body.zw * 0.5 - vec2<f32>(inset), vec2<f32>(2.0));
    var local = p - frame.body.xy - frame.body.zw * 0.5;
    let radius = max(min(frame.shape.x - inset, min(half_size.x, half_size.y)), 0.0);
    if (frame.notch.z > 0.5) {
        // 侧边刘海先按 shape::EdgeMap 的逆变换换成贴顶轮廓。
        if (frame.count.y == 1u) {
            local = local.yx;
            half_size = half_size.yx;
        } else if (frame.count.y == 2u) {
            local = vec2<f32>(local.y, -local.x);
            half_size = half_size.yx;
        } else if (frame.count.y == 3u) {
            local.y = -local.y;
        }
        return notch_sdf(local, half_size, radius, frame.notch.x, frame.notch.y);
    }
    return rr_sdf(local, half_size, radius);
}

fn sample_backdrop(px : vec2<f32>) -> vec3<f32> {
    let valid = frame.tex.zw;
    let scale = valid / frame.tex.xy;
    let uv = clamp(px / valid, vec2<f32>(0.001), vec2<f32>(0.999)) * scale;
    return textureSampleLevel(backdrop, backdrop_sampler, uv, 0.0).rgb;
}

struct DaozaiVertexOut {
    @builtin(position) position : vec4<f32>,
    @location(0) uv : vec2<f32>,
};

@vertex
fn daozai_vs(@builtin(vertex_index) i : u32) -> DaozaiVertexOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return DaozaiVertexOut(vec4<f32>(2.0 * uv.x - 1.0, 1.0 - 2.0 * uv.y, 0.0, 1.0), uv);
}

override daozai_srgb_target : bool = false;

@fragment
fn daozai_fs(i : DaozaiVertexOut) -> @location(0) vec4<f32> {
    let color = material(MaterialIn(i.uv * frame.capture.zw, i.uv));
    if (daozai_srgb_target) {
        // egui 的非 sRGB 目标直接接收 gamma 编码的颜色，sRGB 目标在写入时自行编码。
        let rgb = max(color.rgb, vec3<f32>(0.0));
        let linear = select(pow((rgb + 0.055) / 1.055, vec3<f32>(2.4)), rgb / 12.92, rgb < vec3<f32>(0.04045));
        return vec4<f32>(linear, color.a);
    }
    return color;
}
