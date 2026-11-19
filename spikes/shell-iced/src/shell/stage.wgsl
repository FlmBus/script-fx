// Stand-in patch art for the spike: an audio-reactive "generated layer" with the window's
// rounded corners cut in the shader. Real patches bring their own layers and shaders.

struct Inputs {
    rect: vec4<f32>,     // x, y, w, h in physical pixels (framebuffer space)
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    time: f32,
    level: f32,
    radius: f32,         // corner radius, physical pixels
    scale: f32,
};

@group(0) @binding(0) var<uniform> u: Inputs;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the viewport (the shader widget's bounds).
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let local = pos.xy - u.rect.xy;
    let uv = local / u.rect.zw;
    let aspect = u.rect.z / u.rect.w;
    let p = vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5);
    let t = u.time * 0.25;
    let lvl = clamp(u.level, 0.0, 1.0);

    // Two soft blobs drifting, plus a slow plasma field.
    let b1 = vec2<f32>(-0.55 + 0.15 * sin(t * 1.3), -0.25 + 0.1 * cos(t));
    let b2 = vec2<f32>(0.6 + 0.12 * cos(t * 0.9), 0.2 + 0.12 * sin(t * 1.7));
    let g1 = exp(-dot(p - b1, p - b1) * (3.0 - lvl));
    let g2 = exp(-dot(p - b2, p - b2) * (3.5 - lvl));
    let field = sin(p.x * 3.0 + t * 2.0) + sin(p.y * 4.0 - t * 1.6) + sin((p.x + p.y) * 2.5 + t);

    let base = vec3<f32>(0.035, 0.03, 0.06);
    var col = base + u.color_a.rgb * g1 * (0.55 + 0.8 * lvl) + u.color_b.rgb * g2 * 0.5;
    col += 0.04 * field * mix(u.color_a.rgb, u.color_b.rgb, uv.x);

    // A ring that breathes with the level.
    let ring = abs(length(p) - (0.22 + 0.08 * lvl)) - 0.002;
    col += u.color_a.rgb * exp(-ring * ring * 4000.0) * (0.15 + 0.85 * lvl);

    // Vignette.
    col *= 1.0 - 0.45 * dot(uv - 0.5, uv - 0.5) * 2.0;

    // Rounded window corners, antialiased over one physical pixel.
    let d = rounded_box(local - u.rect.zw * 0.5, u.rect.zw * 0.5, u.radius);
    let a = clamp(0.5 - d, 0.0, 1.0);
    return vec4<f32>(col * a, a); // premultiplied
}
