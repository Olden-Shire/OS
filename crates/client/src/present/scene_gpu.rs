// custom — optional GPU scene renderer (host-side, not part of the gamepack).
//
// Replays the per-frame `SceneFrame` captured by `dash3d::scene_capture` into an
// offscreen FBO, then reads the pixels back so the existing CPU composite path
// (scene image → pix2d → Present) is reused unchanged. Faces are drawn in
// capture order (== the CPU painter's order) with DEPTH_TEST off and blending
// on, so GPU primitive-order reproduces the software painter's algorithm.
//
// Vertices arrive in view space; the vertex shader does only the
// perspective-divide + NDC map (matching `(v*zoom)/vz + origin`), and the
// clip-space near plane is arranged so view_z < 50 clips exactly like Jagex.
//
// Two fragment paths, chosen per-face by the captured `tex` field:
//   * plain  — per-vertex HSL palette index → 256×256 palette texture lookup
//              (bit-identical RGB to the CPU colourTable).
//   * texture — per-vertex light scalar [2,126] modulates a texel from a
//              128×128 TEXTURE_2D_ARRAY layer (one layer per texture id);
//              `texel.rgb * (light/128)` matches the CPU `texel*(light*2)>>8`.
//
// Texture arrays need GLSL ES 300 (sampler2DArray is absent in ES 100), which
// WebGL2 supports alongside the existing ES-100 frame/imgui shaders. Desktop
// uses #version 330 core.

use glow::HasContext;
use std::error::Error;

use super::gl::{as_bytes, link_program, tex_params};
use crate::dash3d::scene_capture::{SceneFrame, VERTEX_FLOATS};

// Texture-array layers: one per texture id. The 2007 cache has well under this
// many textures; ids at/above the cap fall back to untextured (their light
// still shows). 128×128×4 bytes × 128 layers ≈ 8 MB GPU.
const TEX_LAYERS: i32 = 128;
const TEX_SIZE: i32 = 128;

// Shared shader body (valid in both #version 330 core and #version 300 es —
// both use in/out/texture()).
const SCENE_BODY_VS: &str = "
in vec3 a_view; in float a_c; in vec2 a_uv; in float a_tex; in float a_alpha;
out float v_c; out vec2 v_uv; out float v_tex; out float v_alpha;
uniform float u_zoom, u_ox, u_oy, u_hw, u_hh, u_far;
void main() {
  float vz = max(a_view.z, 1.0);
  float sx = (a_view.x * u_zoom) / vz + u_ox;
  float sy = (a_view.y * u_zoom) / vz + u_oy;
  float ndcx = sx / u_hw - 1.0;
  float ndcy = 1.0 - sy / u_hh;
  float zc = (a_view.z - 50.0) / (u_far - 50.0);
  gl_Position = vec4(ndcx * vz, ndcy * vz, (zc * 2.0 - 1.0) * vz, vz);
  v_c = a_c; v_uv = a_uv; v_tex = a_tex; v_alpha = a_alpha;
}";
const SCENE_BODY_FS: &str = "
in float v_c; in vec2 v_uv; in float v_tex; in float v_alpha;
uniform sampler2D u_palette;
uniform sampler2DArray u_atlas;
void main() {
  if (v_tex < 0.0) {
    float idx = clamp(v_c, 0.0, 65535.0);
    float px = mod(idx, 256.0);
    float py = floor(idx / 256.0);
    vec3 rgb = TEXFN(u_palette, (vec2(px, py) + 0.5) / 256.0).rgb;
    OUTCOL = vec4(rgb, v_alpha);
  } else {
    vec4 t = TEXFN(u_atlas, vec3(v_uv, v_tex));
    OUTCOL = vec4(t.rgb * (v_c / 128.0), t.a * v_alpha);
  }
}";

fn vs_src(es: bool) -> String {
    if es {
        format!("#version 300 es\nprecision highp float;\nprecision highp sampler2DArray;{SCENE_BODY_VS}")
    } else {
        format!("#version 330 core{SCENE_BODY_VS}")
    }
}
fn fs_src(es: bool) -> String {
    let body = SCENE_BODY_FS.replace("TEXFN", "texture");
    if es {
        format!("#version 300 es\nprecision highp float;\nprecision highp sampler2DArray;\nout vec4 o_col;\n{}",
            body.replace("OUTCOL", "o_col"))
    } else {
        format!("#version 330 core\nout vec4 o_col;\n{}", body.replace("OUTCOL", "o_col"))
    }
}

pub struct ScenePipeline {
    prog: glow::Program,
    fbo: glow::Framebuffer,
    color_tex: glow::Texture,
    palette_tex: glow::Texture,
    atlas_tex: glow::Texture,
    uploaded: [bool; TEX_LAYERS as usize],
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    size: (i32, i32),
    u_zoom: Option<glow::UniformLocation>,
    u_ox: Option<glow::UniformLocation>,
    u_oy: Option<glow::UniformLocation>,
    u_hw: Option<glow::UniformLocation>,
    u_hh: Option<glow::UniformLocation>,
    u_far: Option<glow::UniformLocation>,
    u_palette: Option<glow::UniformLocation>,
    u_atlas: Option<glow::UniformLocation>,
    rgba: Vec<u8>,
}

impl ScenePipeline {
    pub unsafe fn new(gl: &glow::Context, es: bool) -> Result<Self, Box<dyn Error>> {
        let to_err = |e: String| -> Box<dyn Error> { e.into() };
        unsafe {
            let prog = link_program(gl, &vs_src(es), &fs_src(es), &["a_view", "a_c", "a_uv", "a_tex", "a_alpha"])?;
            let u_zoom = gl.get_uniform_location(prog, "u_zoom");
            let u_ox = gl.get_uniform_location(prog, "u_ox");
            let u_oy = gl.get_uniform_location(prog, "u_oy");
            let u_hw = gl.get_uniform_location(prog, "u_hw");
            let u_hh = gl.get_uniform_location(prog, "u_hh");
            let u_far = gl.get_uniform_location(prog, "u_far");
            let u_palette = gl.get_uniform_location(prog, "u_palette");
            let u_atlas = gl.get_uniform_location(prog, "u_atlas");

            // HSL → RGB palette as a 256×256 RGBA8 texture (index i at texel
            // (i&255, i>>8)), NEAREST so exact-centre sampling returns the
            // precise colourTable entry — bit-identical to the CPU lookup.
            let table = crate::dash3d::pix3d::colour_table();
            let mut px = vec![0u8; 256 * 256 * 4];
            for (i, &rgb) in table.iter().enumerate() {
                let o = i * 4;
                px[o] = ((rgb >> 16) & 0xFF) as u8;
                px[o + 1] = ((rgb >> 8) & 0xFF) as u8;
                px[o + 2] = (rgb & 0xFF) as u8;
                px[o + 3] = 255;
            }
            let palette_tex = gl.create_texture().map_err(to_err)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(palette_tex));
            tex_params(gl, glow::NEAREST);
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32, 256, 256, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(&px)),
            );

            // Texture atlas: a 128×128 RGBA8 2D array, one layer per texture id,
            // filled lazily. NEAREST (RS textures are point-sampled) + clamp.
            let atlas_tex = gl.create_texture().map_err(to_err)?;
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(atlas_tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            // Jagex clamps the U (column) coord but wraps V (row) — the texel
            // index masks the row with 0x3F80 (mod 128) while clamping the
            // column to [0,16256]. Mirror that per-axis.
            gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
            gl.tex_image_3d(
                glow::TEXTURE_2D_ARRAY, 0, glow::RGBA as i32, TEX_SIZE, TEX_SIZE, TEX_LAYERS, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None),
            );

            let fbo = gl.create_framebuffer().map_err(to_err)?;
            let color_tex = gl.create_texture().map_err(to_err)?;
            let vbo = gl.create_buffer().map_err(to_err)?;
            let ebo = gl.create_buffer().map_err(to_err)?;

            Ok(Self {
                prog, fbo, color_tex, palette_tex, atlas_tex,
                uploaded: [false; TEX_LAYERS as usize],
                vbo, ebo, size: (0, 0),
                u_zoom, u_ox, u_oy, u_hw, u_hh, u_far, u_palette, u_atlas,
                rgba: Vec::new(),
            })
        }
    }

    unsafe fn ensure_size(&mut self, gl: &glow::Context, w: i32, h: i32) -> bool {
        if self.size == (w, h) {
            return true;
        }
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.color_tex));
            tex_params(gl, glow::NEAREST);
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32, w, h, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(self.color_tex), 0);
            let ok = gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            if ok {
                self.size = (w, h);
            }
            ok
        }
    }

    // Upload any texture id referenced this frame that isn't resident yet, into
    // its matching array layer. Skips ids that haven't streamed in.
    unsafe fn upload_textures(&mut self, gl: &glow::Context, frame: &SceneFrame) {
        unsafe {
            let mut bound = false;
            let mut i = 0;
            while i < frame.verts.len() {
                let tex = frame.verts[i + 6] as i32;
                i += VERTEX_FLOATS;
                if tex < 0 || tex >= TEX_LAYERS || self.uploaded[tex as usize] {
                    continue;
                }
                let Some(texels) = crate::dash3d::texture_manager::get_texels(tex) else { continue; };
                let opaque = crate::dash3d::texture_manager::is_opaque(tex);
                let mut rgba = vec![0u8; (TEX_SIZE * TEX_SIZE * 4) as usize];
                for (j, &t) in texels.iter().enumerate() {
                    let o = j * 4;
                    if o + 3 >= rgba.len() { break; }
                    // Non-opaque textures use texel 0 as fully transparent.
                    let a = if !opaque && (t & 0xFFFFFF) == 0 { 0 } else { 255 };
                    rgba[o] = ((t >> 16) & 0xFF) as u8;
                    rgba[o + 1] = ((t >> 8) & 0xFF) as u8;
                    rgba[o + 2] = (t & 0xFF) as u8;
                    rgba[o + 3] = a;
                }
                if !bound {
                    gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas_tex));
                    bound = true;
                }
                gl.tex_sub_image_3d(
                    glow::TEXTURE_2D_ARRAY, 0, 0, 0, tex, TEX_SIZE, TEX_SIZE, 1,
                    glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(&rgba)),
                );
                self.uploaded[tex as usize] = true;
            }
        }
    }

    /// Render `frame` to the offscreen FBO and read the pixels back into `out`
    /// (resized to w*h, 0x00RRGGBB, top-left origin). Returns false on any GL
    /// failure so the caller can fall back to the CPU rasterizer.
    pub unsafe fn render(&mut self, gl: &glow::Context, frame: &SceneFrame, out: &mut Vec<u32>) -> bool {
        let (w, h) = (frame.w, frame.h);
        if w <= 0 || h <= 0 {
            return false;
        }
        unsafe {
            if !self.ensure_size(gl, w, h) {
                return false;
            }
            self.upload_textures(gl, frame);

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.viewport(0, 0, w, h);
            gl.disable(glow::DEPTH_TEST);
            gl.depth_mask(false);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::SCISSOR_TEST);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            if !frame.indices.is_empty() {
                gl.use_program(Some(self.prog));
                gl.uniform_1_f32(self.u_zoom.as_ref(), frame.zoom as f32);
                gl.uniform_1_f32(self.u_ox.as_ref(), frame.origin_x as f32);
                gl.uniform_1_f32(self.u_oy.as_ref(), frame.origin_y as f32);
                gl.uniform_1_f32(self.u_hw.as_ref(), (w as f32) / 2.0);
                gl.uniform_1_f32(self.u_hh.as_ref(), (h as f32) / 2.0);
                gl.uniform_1_f32(self.u_far.as_ref(), 100_000.0);
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(self.palette_tex));
                gl.uniform_1_i32(self.u_palette.as_ref(), 0);
                gl.active_texture(glow::TEXTURE1);
                gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas_tex));
                gl.uniform_1_i32(self.u_atlas.as_ref(), 1);

                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(&frame.verts), glow::STREAM_DRAW);
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ebo));
                gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, as_bytes(&frame.indices), glow::STREAM_DRAW);

                let stride = (VERTEX_FLOATS * 4) as i32;
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
                gl.enable_vertex_attrib_array(1);
                gl.vertex_attrib_pointer_f32(1, 1, glow::FLOAT, false, stride, 12);
                gl.enable_vertex_attrib_array(2);
                gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, 16);
                gl.enable_vertex_attrib_array(3);
                gl.vertex_attrib_pointer_f32(3, 1, glow::FLOAT, false, stride, 24);
                gl.enable_vertex_attrib_array(4);
                gl.vertex_attrib_pointer_f32(4, 1, glow::FLOAT, false, stride, 28);

                gl.draw_elements(glow::TRIANGLES, frame.indices.len() as i32, glow::UNSIGNED_INT, 0);

                gl.disable_vertex_attrib_array(1);
                gl.disable_vertex_attrib_array(2);
                gl.disable_vertex_attrib_array(3);
                gl.disable_vertex_attrib_array(4);
                gl.disable(glow::BLEND);
            }

            let n = (w * h) as usize;
            self.rgba.resize(n * 4, 0);
            gl.read_pixels(
                0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut self.rgba)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.depth_mask(true);

            // GL origin is bottom-left; the scene image is top-left. Flip rows
            // and pack to 0x00RRGGBB.
            out.resize(n, 0);
            let wu = w as usize;
            let hu = h as usize;
            for row in 0..hu {
                let src = (hu - 1 - row) * wu * 4;
                let dst = row * wu;
                for col in 0..wu {
                    let s = src + col * 4;
                    out[dst + col] = ((self.rgba[s] as u32) << 16)
                        | ((self.rgba[s + 1] as u32) << 8)
                        | (self.rgba[s + 2] as u32);
                }
            }
        }
        true
    }
}
