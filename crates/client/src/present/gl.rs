// custom — glow Present backend: the game frame is uploaded to a texture
// each frame and stretched by the GPU; the imgui overlay renders through GL
// at native window resolution.
//
// glow abstracts desktop GL, GLES and WebGL behind one API. Context
// creation is the only per-platform part:
//   native — glutin (WGL/EGL/GLX), GL with a GLES fallback
//   wasm   — WebGL2 from the winit canvas
// Every shader has a GLSL 330 (desktop core) and GLSL 100 (GLES2/WebGL)
// variant selected at runtime, so the draw code below is shared verbatim.

use std::error::Error;
use std::rc::Rc;

use glow::HasContext;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

#[cfg(not(target_arch = "wasm32"))]
use std::num::NonZeroU32;

#[cfg(not(target_arch = "wasm32"))]
use glutin::config::ConfigTemplateBuilder;
#[cfg(not(target_arch = "wasm32"))]
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext};
#[cfg(not(target_arch = "wasm32"))]
use glutin::display::GetGlDisplay;
#[cfg(not(target_arch = "wasm32"))]
use glutin::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use glutin::surface::{Surface, SwapInterval, WindowSurface};
#[cfg(not(target_arch = "wasm32"))]
use glutin_winit::{DisplayBuilder, GlWindow};
#[cfg(not(target_arch = "wasm32"))]
use winit::raw_window_handle::HasWindowHandle;

use crate::imgui_overlay::PerfOverlay;
use crate::perf;

// ── Shaders ─────────────────────────────────────────────────────────────
// The game frame is 0x00RRGGBB u32s uploaded raw as RGBA bytes, so the
// sampler sees (B, G, R, x) — the fragment shader swizzles .bgr back.

const FRAME_VS_330: &str = "#version 330 core
in vec2 a_pos; in vec2 a_uv; out vec2 v_uv;
void main() { v_uv = a_uv; gl_Position = vec4(a_pos, 0.0, 1.0); }";
const FRAME_FS_330: &str = "#version 330 core
in vec2 v_uv; out vec4 o_col; uniform sampler2D u_tex;
void main() { vec4 c = texture(u_tex, v_uv); o_col = vec4(c.bgr, 1.0); }";

const FRAME_VS_100: &str = "attribute vec2 a_pos; attribute vec2 a_uv; varying vec2 v_uv;
void main() { v_uv = a_uv; gl_Position = vec4(a_pos, 0.0, 1.0); }";
const FRAME_FS_100: &str = "precision mediump float; varying vec2 v_uv; uniform sampler2D u_tex;
void main() { vec4 c = texture2D(u_tex, v_uv); gl_FragColor = vec4(c.bgr, 1.0); }";

const IM_VS_330: &str = "#version 330 core
in vec2 a_pos; in vec2 a_uv; in vec4 a_col;
out vec2 v_uv; out vec4 v_col;
uniform vec2 u_scale; uniform vec2 u_trans;
void main() { v_uv = a_uv; v_col = a_col;
  gl_Position = vec4(a_pos * u_scale + u_trans, 0.0, 1.0); }";
const IM_FS_330: &str = "#version 330 core
in vec2 v_uv; in vec4 v_col; out vec4 o_col; uniform sampler2D u_tex;
void main() { o_col = v_col * texture(u_tex, v_uv); }";

const IM_VS_100: &str = "attribute vec2 a_pos; attribute vec2 a_uv; attribute vec4 a_col;
varying vec2 v_uv; varying vec4 v_col;
uniform vec2 u_scale; uniform vec2 u_trans;
void main() { v_uv = a_uv; v_col = a_col;
  gl_Position = vec4(a_pos * u_scale + u_trans, 0.0, 1.0); }";
const IM_FS_100: &str = "precision mediump float;
varying vec2 v_uv; varying vec4 v_col; uniform sampler2D u_tex;
void main() { gl_FragColor = v_col * texture2D(u_tex, v_uv); }";

// GL objects shared by every platform once a context exists.
struct Pipeline {
    frame_prog: glow::Program,
    frame_tex: glow::Texture,
    quad_vbo: glow::Buffer,
    _vao: Option<glow::VertexArray>,
    im_prog: glow::Program,
    im_u_scale: Option<glow::UniformLocation>,
    im_u_trans: Option<glow::UniformLocation>,
    im_vbo: glow::Buffer,
    im_ebo: glow::Buffer,
}

pub struct GlPresent {
    #[cfg(not(target_arch = "wasm32"))]
    surface: Surface<WindowSurface>,
    #[cfg(not(target_arch = "wasm32"))]
    context: PossiblyCurrentContext,
    // The canvas backing store doesn't track the window size by itself —
    // present() syncs width/height attributes to the physical size.
    #[cfg(target_arch = "wasm32")]
    canvas: web_sys::HtmlCanvasElement,
    gl: glow::Context,
    pipe: Pipeline,
    frame_tex_size: (u32, u32),
    font_tex: Option<glow::Texture>,
    // GLES/WebGL2 shader variant flag + the lazily-built optional GPU scene
    // pipeline (only allocated the first time the gpu_scene toggle renders).
    es: bool,
    scene_pipe: Option<super::scene_gpu::ScenePipeline>,
}

impl GlPresent {
    // Native: build the window TOGETHER with the GL config (required on
    // some platforms), so this returns the window for the caller to keep.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create(
        event_loop: &ActiveEventLoop,
        attrs: WindowAttributes,
    ) -> Result<(Rc<Window>, Self), Box<dyn Error>> {
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(event_loop, ConfigTemplateBuilder::new(), |mut configs| {
                configs.next().expect("no GL configs")
            })?;
        let window = Rc::new(window.ok_or("DisplayBuilder returned no window")?);
        let display = config.display();
        let rwh = window.window_handle()?.as_raw();

        // Plain GL first, GLES as fallback (some drivers expose only one).
        let not_current = unsafe {
            display
                .create_context(&config, &ContextAttributesBuilder::new().build(Some(rwh)))
                .or_else(|_| {
                    display.create_context(
                        &config,
                        &ContextAttributesBuilder::new()
                            .with_context_api(ContextApi::Gles(None))
                            .build(Some(rwh)),
                    )
                })?
        };
        let surface_attrs = window.build_surface_attributes(Default::default())?;
        let surface = unsafe { display.create_window_surface(&config, &surface_attrs)? };
        let context = not_current.make_current(&surface)?;
        // The mainloop self-paces at 50Hz; don't also block on vsync.
        let _ = surface.set_swap_interval(&context, SwapInterval::DontWait);

        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s).cast())
        };
        let es = gl.version().is_embedded;
        let pipe = unsafe { Pipeline::new(&gl, es)? };
        Ok((
            window,
            Self {
                surface,
                context,
                gl,
                pipe,
                frame_tex_size: (0, 0),
                font_tex: None,
                es,
                scene_pipe: None,
            },
        ))
    }

    // Wasm: winit owns a canvas; WebGL2 comes straight off it. The GLSL
    // 100 shader set runs unchanged on WebGL2.
    #[cfg(target_arch = "wasm32")]
    pub fn create(
        event_loop: &ActiveEventLoop,
        attrs: WindowAttributes,
    ) -> Result<(Rc<Window>, Self), Box<dyn Error>> {
        use wasm_bindgen::JsCast;
        use winit::platform::web::WindowExtWebSys;

        let window = Rc::new(event_loop.create_window(attrs).map_err(|e| e.to_string())?);
        let canvas = window.canvas().ok_or("winit window has no canvas")?;
        // winit creates the canvas but the page must host it.
        let doc = web_sys::window()
            .and_then(|w| w.document())
            .ok_or("no document")?;
        let mount = doc
            .get_element_by_id("game")
            .or_else(|| doc.body().map(|b| b.into()))
            .ok_or("no mount point")?;
        mount
            .append_child(&canvas)
            .map_err(|e| format!("{e:?}"))?;

        let webgl2 = canvas
            .get_context("webgl2")
            .map_err(|e| format!("{e:?}"))?
            .ok_or("webgl2 unavailable")?
            .dyn_into::<web_sys::WebGl2RenderingContext>()
            .map_err(|_| "webgl2 context cast failed")?;
        let gl = glow::Context::from_webgl2_context(webgl2);
        let pipe = unsafe { Pipeline::new(&gl, true)? };
        Ok((
            window,
            Self {
                canvas,
                gl,
                pipe,
                frame_tex_size: (0, 0),
                font_tex: None,
                es: true,
                scene_pipe: None,
            },
        ))
    }

    // Upload the overlay's font atlas once the overlay exists (it needs the
    // window's scale factor, so it's created after the window).
    pub fn attach_overlay_fonts(&mut self, overlay: &mut PerfOverlay) {
        let (rgba, w, h) = overlay.build_rgba_font_atlas();
        if w == 0 || h == 0 {
            return; // wasm overlay stub — no fonts, nothing to draw
        }
        let gl = &self.gl;
        unsafe {
            let tex = match gl.create_texture() {
                Ok(t) => t,
                Err(_) => return,
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            tex_params(gl, glow::LINEAR);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                w as i32,
                h as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&rgba)),
            );
            self.font_tex = Some(tex);
        }
    }

    unsafe fn draw_imgui(&self, draw_data: &imgui::DrawData, win_w: u32, win_h: u32) {
        let Some(font_tex) = self.font_tex else { return };
        let gl = &self.gl;
        let pipe = &self.pipe;
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.enable(glow::SCISSOR_TEST);
            gl.use_program(Some(pipe.im_prog));
            // Screen px → NDC.
            gl.uniform_2_f32(
                pipe.im_u_scale.as_ref(),
                2.0 / win_w as f32,
                -2.0 / win_h as f32,
            );
            gl.uniform_2_f32(pipe.im_u_trans.as_ref(), -1.0, 1.0);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(font_tex));

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(pipe.im_vbo));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(pipe.im_ebo));
            // imgui::DrawVert { pos: [f32; 2], uv: [f32; 2], col: [u8; 4] }
            let stride = std::mem::size_of::<imgui::DrawVert>() as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 8);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::UNSIGNED_BYTE, true, stride, 16);

            for list in draw_data.draw_lists() {
                let vtx = list.vtx_buffer();
                let idx = list.idx_buffer();
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(vtx), glow::STREAM_DRAW);
                gl.buffer_data_u8_slice(
                    glow::ELEMENT_ARRAY_BUFFER,
                    as_bytes(idx),
                    glow::STREAM_DRAW,
                );
                for cmd in list.commands() {
                    if let imgui::DrawCmd::Elements { count, cmd_params } = cmd {
                        let c = cmd_params.clip_rect;
                        let x = c[0].max(0.0) as i32;
                        let y = (win_h as f32 - c[3]).max(0.0) as i32; // GL origin: bottom-left
                        let w = (c[2].min(win_w as f32) - c[0]) as i32;
                        let h = (c[3].min(win_h as f32) - c[1]) as i32;
                        if w <= 0 || h <= 0 {
                            continue;
                        }
                        gl.scissor(x, y, w, h);
                        gl.draw_elements(
                            glow::TRIANGLES,
                            count as i32,
                            glow::UNSIGNED_SHORT,
                            (cmd_params.idx_offset * 2) as i32,
                        );
                    }
                }
            }

            gl.disable_vertex_attrib_array(2);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
        }
    }
}

impl super::Present for GlPresent {
    fn name(&self) -> &'static str {
        "gl"
    }

    fn render_scene(
        &mut self,
        frame: &crate::dash3d::scene_capture::SceneFrame,
        out: &mut Vec<u32>,
    ) -> bool {
        // Lazily build the scene pipeline on first use; if it fails to compile,
        // disable the path for the rest of the session (returns false → CPU).
        if self.scene_pipe.is_none() {
            match unsafe { super::scene_gpu::ScenePipeline::new(&self.gl, self.es) } {
                Ok(p) => self.scene_pipe = Some(p),
                Err(e) => {
                    eprintln!("[present] gpu scene pipeline failed to init: {e} — using CPU");
                    return false;
                }
            }
        }
        let pipe = self.scene_pipe.as_mut().unwrap();
        unsafe { pipe.render(&self.gl, frame, out) }
    }

    fn resize(&mut self, width: u32, height: u32) {
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            self.surface.resize(&self.context, w, h);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (width, height); // canvas backbuffer follows winit
    }

    fn present(
        &mut self,
        frame: &[u32],
        fw: u32,
        fh: u32,
        win_w: u32,
        win_h: u32,
        overlay: &mut PerfOverlay,
        mouse: (f32, f32),
        buttons: (bool, bool),
    ) {
        if win_w == 0 || win_h == 0 {
            return;
        }
        // Keep the canvas drawing buffer at the physical window size —
        // CSS stretches the element, but the buffer doesn't follow it.
        #[cfg(target_arch = "wasm32")]
        {
            if self.canvas.width() != win_w {
                self.canvas.set_width(win_w);
            }
            if self.canvas.height() != win_h {
                self.canvas.set_height(win_h);
            }
        }
        let gl = &self.gl;
        unsafe {
            let _t = perf::scope(perf::Scope::Blit);
            // Black borders for the vanilla (1:1 top-centre) layout.
            gl.viewport(0, 0, win_w as i32, win_h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            // Place the game quad via the viewport — the quad itself is a
            // full-NDC strip, so the viewport IS the destination rect.
            // GL's viewport origin is bottom-left; flip the y.
            let (dx, dy, dw, dh) = super::layout_rect(fw, fh, win_w, win_h);
            gl.viewport(dx, win_h as i32 - dy - dh as i32, dw as i32, dh as i32);

            // Upload the finished game frame and stretch it across the
            // window. (Re)allocate the texture only when the size changes.
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.pipe.frame_tex));
            let bytes = as_bytes(frame);
            if self.frame_tex_size != (fw, fh) {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    fw as i32,
                    fh as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(bytes)),
                );
                self.frame_tex_size = (fw, fh);
            } else {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    fw as i32,
                    fh as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(bytes)),
                );
            }

            gl.use_program(Some(self.pipe.frame_prog));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.pipe.quad_vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);

            // Overlay draws in window space — restore the full viewport.
            gl.viewport(0, 0, win_w as i32, win_h as i32);
        }

        overlay.frame_with(win_w, win_h, mouse, buttons, |draw_data, _atlas| unsafe {
            self.draw_imgui(draw_data, win_w, win_h);
        });

        // The browser presents the canvas after the rAF callback; only
        // native double-buffering needs an explicit flip.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _t = perf::scope(perf::Scope::Blit);
            let _ = self.surface.swap_buffers(&self.context);
        }
    }
}

impl Pipeline {
    unsafe fn new(gl: &glow::Context, es: bool) -> Result<Self, Box<dyn Error>> {
        unsafe {
            // Desktop core profiles refuse to draw without a bound VAO;
            // GLES2/WebGL don't require one (WebGL2 has them, but the
            // per-draw pointer setup below works either way). One VAO
            // bound forever covers both draw layouts.
            let vao = gl.create_vertex_array().ok();
            gl.bind_vertex_array(vao);

            let frame_prog = link_program(
                gl,
                if es { FRAME_VS_100 } else { FRAME_VS_330 },
                if es { FRAME_FS_100 } else { FRAME_FS_330 },
                &["a_pos", "a_uv"],
            )?;
            let im_prog = link_program(
                gl,
                if es { IM_VS_100 } else { IM_VS_330 },
                if es { IM_FS_100 } else { IM_FS_330 },
                &["a_pos", "a_uv", "a_col"],
            )?;
            let im_u_scale = gl.get_uniform_location(im_prog, "u_scale");
            let im_u_trans = gl.get_uniform_location(im_prog, "u_trans");

            // Fullscreen strip: x, y, u, v. Frame row 0 is the top of the
            // game image, so NDC y=+1 maps to v=0.
            let quad: [f32; 16] = [
                -1.0, 1.0, 0.0, 0.0, //
                1.0, 1.0, 1.0, 0.0, //
                -1.0, -1.0, 0.0, 1.0, //
                1.0, -1.0, 1.0, 1.0,
            ];
            let quad_vbo = gl.create_buffer().map_err(err)?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(&quad), glow::STATIC_DRAW);

            let frame_tex = gl.create_texture().map_err(err)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(frame_tex));
            // Nearest matches the softbuffer stretch (crisp pixels).
            tex_params(gl, glow::NEAREST);

            let im_vbo = gl.create_buffer().map_err(err)?;
            let im_ebo = gl.create_buffer().map_err(err)?;

            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);

            Ok(Self {
                frame_prog,
                frame_tex,
                quad_vbo,
                _vao: vao,
                im_prog,
                im_u_scale,
                im_u_trans,
                im_vbo,
                im_ebo,
            })
        }
    }
}

// ── small helpers ───────────────────────────────────────────────────────

fn err(e: String) -> Box<dyn Error> {
    e.into()
}

pub(super) fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), std::mem::size_of_val(v)) }
}

pub(super) unsafe fn tex_params(gl: &glow::Context, filter: u32) {
    unsafe {
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filter as i32);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
    }
}

pub(super) unsafe fn link_program(
    gl: &glow::Context,
    vs_src: &str,
    fs_src: &str,
    attribs: &[&str],
) -> Result<glow::Program, Box<dyn Error>> {
    unsafe {
        let compile = |kind: u32, src: &str| -> Result<glow::Shader, Box<dyn Error>> {
            let sh = gl.create_shader(kind).map_err(err)?;
            gl.shader_source(sh, src);
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(format!("shader: {}", gl.get_shader_info_log(sh)).into());
            }
            Ok(sh)
        };
        let vs = compile(glow::VERTEX_SHADER, vs_src)?;
        let fs = compile(glow::FRAGMENT_SHADER, fs_src)?;
        let prog = gl.create_program().map_err(err)?;
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        // Fixed locations work for both GLSL variants (no layout()
        // qualifiers needed, GLES2 doesn't support them anyway).
        for (i, name) in attribs.iter().enumerate() {
            gl.bind_attrib_location(prog, i as u32, name);
        }
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(format!("link: {}", gl.get_program_info_log(prog)).into());
        }
        gl.detach_shader(prog, vs);
        gl.detach_shader(prog, fs);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        Ok(prog)
    }
}
