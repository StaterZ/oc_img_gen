struct RGB8 { r: u32, g: u32, b: u32; };
struct Output { id: u32, bg: RGB8, fg: RGB8, score: i32; };

@group(0) @binding(0) var<storage, read> pixels: array<RGB8>;
@group(0) @binding(1) var<storage, read_write> outputs: array<Output>;

const WIDTH: u32 = 2u;
const HEIGHT: u32 = 4u;
const BITS: u32 = WIDTH * HEIGHT;

fn perceptual_delta(a: RGB8, b: RGB8) -> u32 {
    let dr = abs(i32(a.r) - i32(b.r));
    let dg = abs(i32(a.g) - i32(b.g));
    let db = abs(i32(a.b) - i32(b.b));
    return u32(dr + dg + db);
}

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let group = gid.x;
    var bg_sum = vec3<u32>(0u);
    var fg_sum = vec3<u32>(0u);
    var bg_count = 0u;
    var fg_count = 0u;

    for (var i = 0u; i < BITS; i++) {
        let pix = pixels[i];
        if ((group >> i) & 1u) == 0u {
            bg_sum += vec3<u32>(pix.r, pix.g, pix.b);
            bg_count++;
        } else {
            fg_sum += vec3<u32>(pix.r, pix.g, pix.b);
            fg_count++;
        }
    }

    let bg = RGB8(
        r: (bg_sum.x / max(bg_count, 1u)),
        g: (bg_sum.y / max(bg_count, 1u)),
        b: (bg_sum.z / max(bg_count, 1u))
    );

    let fg = RGB8(
        r: (fg_sum.x / max(fg_count, 1u)),
        g: (fg_sum.y / max(fg_count, 1u)),
        b: (fg_sum.z / max(fg_count, 1u))
    );

    let score = i32(perceptual_delta(bg, fg));
    outputs[group] = Output(id: group, bg: bg, fg: fg, score: score);
}
