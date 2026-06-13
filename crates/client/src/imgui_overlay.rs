// custom — imgui benchmark overlay (not part of the gamepack).
//
// Renders a small frame-time breakdown graph on top of the presented frame.
// imgui itself is platform-agnostic (it only produces vertex lists); we feed
// its IO from winit events and rasterize its draw data in software straight
// onto the softbuffer surface, so there is no GPU backend to port — the same
// path runs anywhere the client runs (Android included).
//
// Drawn at native window resolution AFTER the game frame is stretched, so
// the overlay text stays crisp at any window size.

#![allow(dead_code)]

use crate::host::Instant;

use imgui::{Condition, Context, FontConfig, FontSource, WindowFlags};

use crate::perf::{self, Scope, HISTORY, SCOPE_COUNT};

// Display series (derived from perf scopes; "ui 2d" is chrome minus the
// nested scene/minimap scopes so the stack adds up to real frame cost).
const SERIES: usize = 6;
const SERIES_NAMES: [&str; SERIES] = ["logic", "scene 3d", "minimap", "ui 2d", "blit", "overlay"];
const SERIES_COLORS: [[f32; 4]; SERIES] = [
    [0.31, 0.76, 0.97, 1.0], // logic — sky blue
    [0.51, 0.78, 0.52, 1.0], // scene 3d — green
    [1.00, 0.84, 0.31, 1.0], // minimap — amber
    [0.73, 0.41, 0.78, 1.0], // ui 2d — purple
    [1.00, 0.54, 0.40, 1.0], // blit — orange
    [0.56, 0.64, 0.68, 1.0], // overlay — grey
];

// Frame budget: targeting 50 fps for now (the gamepack's native 50Hz
// mainloop) — 20ms per frame. The header's "free" readout is what's left
// of this after all measured work.
const BUDGET_MS: f32 = 20.0;

pub struct PerfOverlay {
    ctx: Context,
    // Font atlas copied out as alpha8.
    atlas: Vec<u8>,
    atlas_w: usize,
    atlas_h: usize,
    // Window scale factor the fonts/style were built at (hi-DPI crispness).
    sf: f32,
    last_frame: Instant,
    // Whether imgui wants the mouse (cursor over / dragging the overlay);
    // the host suppresses game clicks while true.
    pub want_mouse: bool,
}

impl PerfOverlay {
    pub fn new(scale_factor: f32) -> Self {
        let mut ctx = Context::create();
        ctx.set_ini_filename(None);
        ctx.io_mut().display_size = [1.0, 1.0];

        let sf = scale_factor.max(1.0);
        ctx.fonts().add_font(&[FontSource::DefaultFontData {
            config: Some(FontConfig {
                size_pixels: 13.0 * sf,
                ..FontConfig::default()
            }),
        }]);

        let style = ctx.style_mut();
        style.window_rounding = 6.0;
        style.window_border_size = 1.0;
        style.window_padding = [10.0, 8.0];
        style.item_spacing = [8.0, 3.0];
        style.scale_all_sizes(sf);
        style.colors[imgui::StyleColor::WindowBg as usize] = [0.06, 0.07, 0.09, 0.88];
        style.colors[imgui::StyleColor::Border as usize] = [1.0, 1.0, 1.0, 0.08];
        style.colors[imgui::StyleColor::Text as usize] = [0.92, 0.93, 0.94, 1.0];

        let (atlas, atlas_w, atlas_h) = {
            let fonts = ctx.fonts();
            let tex = fonts.build_alpha8_texture();
            (tex.data.to_vec(), tex.width as usize, tex.height as usize)
        };

        Self {
            ctx,
            atlas,
            atlas_w,
            atlas_h,
            sf,
            last_frame: Instant::now(),
            want_mouse: false,
        }
    }

    // RGBA32 copy of the font atlas for GPU backends (uploaded once at
    // backend init; the alpha8 copy stays for the software rasterizer).
    pub fn build_rgba_font_atlas(&mut self) -> (Vec<u8>, u32, u32) {
        let tex = self.ctx.fonts().build_rgba32_texture();
        (tex.data.to_vec(), tex.width, tex.height)
    }

    // Build one overlay frame (`dw` x `dh` = window physical pixels;
    // `mouse` is the raw window cursor; `buttons` is (left, right)), then
    // hand the finished imgui draw data to `render` — the active Present
    // backend supplies the renderer (software raster or GL). The callback
    // also receives the alpha8 font atlas for the software path.
    pub fn frame_with(
        &mut self,
        dw: u32,
        dh: u32,
        mouse: (f32, f32),
        buttons: (bool, bool),
        render: impl FnOnce(&imgui::DrawData, (&[u8], usize, usize)),
    ) {
        if !perf::overlay_visible() {
            self.want_mouse = false;
            return;
        }
        let _t = perf::scope(Scope::Imgui);

        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        let sf = self.sf;
        {
            let io = self.ctx.io_mut();
            io.display_size = [dw as f32, dh as f32];
            io.delta_time = dt.max(1.0 / 1000.0);
            io.mouse_pos = [mouse.0, mouse.1];
            io.mouse_down[0] = buttons.0;
            io.mouse_down[1] = buttons.1;
        }

        let (hist, frame_interval) = perf::snapshot();
        // perf scopes → display series (ui 2d by subtraction).
        let derive = |row: &[f32; SCOPE_COUNT]| -> [f32; SERIES] {
            let ui2d = (row[Scope::Chrome as usize]
                - row[Scope::Scene as usize]
                - row[Scope::Minimap as usize])
                .max(0.0);
            [
                row[Scope::Logic as usize],
                row[Scope::Scene as usize],
                row[Scope::Minimap as usize],
                ui2d,
                row[Scope::Blit as usize],
                row[Scope::Imgui as usize],
            ]
        };

        // Averages over the most recent 30 frames for the legend.
        let mut avg = [0.0f32; SERIES];
        let tail = &hist[hist.len().saturating_sub(30)..];
        for row in tail {
            let d = derive(row);
            for s in 0..SERIES {
                avg[s] += d[s];
            }
        }
        for v in avg.iter_mut() {
            *v /= tail.len().max(1) as f32;
        }
        let work_ms: f32 = avg.iter().sum();
        let fps = if frame_interval > 0.0 {
            1000.0 / frame_interval
        } else {
            0.0
        };

        // Peak stacked total over the window sets the graph scale (never
        // below the 20ms budget so the budget line stays in frame).
        let peak = hist
            .iter()
            .map(|r| derive(r).iter().sum::<f32>())
            .fold(BUDGET_MS * 1.1, f32::max);

        let graph_w = HISTORY as f32 * sf;
        let graph_h = 64.0 * sf;

        let ui = self.ctx.new_frame();
        ui.window("perf")
            .flags(
                WindowFlags::NO_TITLE_BAR
                    | WindowFlags::ALWAYS_AUTO_RESIZE
                    | WindowFlags::NO_SCROLLBAR
                    | WindowFlags::NO_COLLAPSE
                    | WindowFlags::NO_NAV,
            )
            .position([12.0, 12.0], Condition::FirstUseEver)
            .build(|| {
                let free_ms = BUDGET_MS - work_ms;
                let budget_col = if free_ms >= 0.0 {
                    [0.51, 0.78, 0.52, 1.0]
                } else {
                    [0.95, 0.36, 0.36, 1.0]
                };
                ui.text_colored(budget_col, format!("{work_ms:5.2} ms"));
                ui.same_line();
                ui.text("work ·");
                ui.same_line();
                ui.text_colored(budget_col, format!("{free_ms:5.2} ms"));
                ui.same_line();
                ui.text(format!("free · {fps:3.0} fps"));

                // ── Stacked frame-time graph ──────────────────────────
                let origin = ui.cursor_screen_pos();
                let dl = ui.get_window_draw_list();
                dl.add_rect(
                    origin,
                    [origin[0] + graph_w, origin[1] + graph_h],
                    [1.0, 1.0, 1.0, 0.04],
                )
                .filled(true)
                .build();
                let scale = graph_h / peak;
                for (i, row) in hist.iter().enumerate() {
                    let d = derive(row);
                    let x = origin[0] + i as f32 * sf;
                    let mut y = origin[1] + graph_h;
                    for s in 0..SERIES {
                        let h = d[s] * scale;
                        if h <= 0.0 {
                            continue;
                        }
                        dl.add_rect([x, y - h], [x + sf, y], SERIES_COLORS[s])
                            .filled(true)
                            .build();
                        y -= h;
                    }
                }
                // 20ms budget line.
                let by = origin[1] + graph_h - BUDGET_MS * scale;
                dl.add_line(
                    [origin[0], by],
                    [origin[0] + graph_w, by],
                    [1.0, 1.0, 1.0, 0.35],
                )
                .build();
                ui.dummy([graph_w, graph_h + 2.0]);

                // ── Legend: swatch + name + avg ms, two columns ───────
                for s in 0..SERIES {
                    let pos = ui.cursor_screen_pos();
                    let sw = 8.0 * sf;
                    dl.add_rect(
                        [pos[0], pos[1] + 3.0 * sf],
                        [pos[0] + sw, pos[1] + 3.0 * sf + sw],
                        SERIES_COLORS[s],
                    )
                    .filled(true)
                    .build();
                    ui.dummy([sw, sw]);
                    ui.same_line();
                    ui.text(format!("{:<8} {:5.2}", SERIES_NAMES[s], avg[s]));
                    if s % 2 == 0 {
                        ui.same_line_with_pos(graph_w * 0.52);
                    }
                }

                // ── Renderer debug toggles (defaults = vanilla 1:1) ──
                ui.separator();
                let mut drag = crate::debug_opts::middle_drag_camera();
                if ui.checkbox("middle-mouse camera rotation", &mut drag) {
                    crate::debug_opts::set_middle_drag_camera(drag);
                }
                let mut zoom = crate::debug_opts::wheel_zoom();
                if ui.checkbox("scroll-wheel zoom", &mut zoom) {
                    crate::debug_opts::set_wheel_zoom(zoom);
                }
                let mut sky = crate::debug_opts::skybox();
                if ui.checkbox("skybox (blue sky gradient)", &mut sky) {
                    crate::debug_opts::set_skybox(sky);
                }
                let mut int_scale = crate::debug_opts::integer_scale();
                if ui.checkbox("integer scaling (lossless)", &mut int_scale) {
                    crate::debug_opts::set_integer_scale(int_scale);
                }
                let mut stretched = crate::debug_opts::stretched();
                if ui.checkbox("stretch frame to window", &mut stretched) {
                    crate::debug_opts::set_stretched(stretched);
                }
                let mut roofs = crate::debug_opts::always_hide_roofs();
                if ui.checkbox("always hide roofs", &mut roofs) {
                    crate::debug_opts::set_always_hide_roofs(roofs);
                }
                let mut extended = crate::debug_opts::extended_draw();
                if ui.checkbox("extended draw distance", &mut extended) {
                    crate::debug_opts::set_extended_draw(extended);
                }
            });

        let draw_data = self.ctx.render();

        // imgui hides auto-resize windows on their first appearing frame
        // (it needs one pass to size them), leaving zero draw lists — and
        // imgui-rs 0.12's draw_lists() builds a slice from the ImVector's
        // then-null pointer, tripping Rust's UB checks. Skip the empty case.
        if draw_data.draw_lists_count() > 0 {
            render(draw_data, (&self.atlas, self.atlas_w, self.atlas_h));
        }

        self.want_mouse = self.ctx.io().want_capture_mouse;
    }
}

// Software renderer for imgui draw data — used by the softbuffer Present
// backend. `atlas` is the alpha8 font atlas from PerfOverlay.
pub fn raster_draw_data(
    draw_data: &imgui::DrawData,
    atlas: (&[u8], usize, usize),
    dst: &mut [u32],
    dw: u32,
    dh: u32,
) {
    for list in draw_data.draw_lists() {
        let vtx = list.vtx_buffer();
        let idx = list.idx_buffer();
        for cmd in list.commands() {
            if let imgui::DrawCmd::Elements { count, cmd_params } = cmd {
                let clip = cmd_params.clip_rect;
                let clip_x0 = (clip[0].max(0.0)) as i32;
                let clip_y0 = (clip[1].max(0.0)) as i32;
                let clip_x1 = (clip[2].min(dw as f32)) as i32;
                let clip_y1 = (clip[3].min(dh as f32)) as i32;
                if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
                    continue;
                }
                let base = cmd_params.idx_offset;
                let clip_i = (clip_x0, clip_y0, clip_x1, clip_y1);
                let tri_count = count / 3;
                let mut tri = 0;
                while tri < tri_count {
                    let at = |k: usize| idx[base + tri * 3 + k] as usize + cmd_params.vtx_offset;
                    let (i0, i1, i2) = (at(0), at(1), at(2));
                    // imgui emits every rectangle (solid fills, graph
                    // bars, glyph quads) as the index pattern
                    // {a,b,c, a,c,d}. Catch the pair and span-fill it —
                    // this covers nearly all of the overlay's pixel area
                    // and is ~10x cheaper than barycentric rasterization.
                    if tri + 1 < tri_count {
                        let (j0, j1, j2) = (at(3), at(4), at(5));
                        if j0 == i0
                            && j1 == i2
                            && fill_axis_rect(
                                dst,
                                dw as i32,
                                clip_i,
                                atlas,
                                [vtx[i0], vtx[i1], vtx[i2], vtx[j2]],
                            )
                        {
                            tri += 2;
                            continue;
                        }
                    }
                    raster_tri(dst, dw as i32, clip_i, atlas, vtx[i0], vtx[i1], vtx[i2]);
                    tri += 1;
                }
            }
        }
    }
}

// Integer src-over: blend src (r|b packed and g shifted) into dst by
// a8 ∈ 0..=255. The r|b channels ride in one multiply.
#[inline(always)]
fn blend_px(d: u32, srb: u32, sg: u32, a8: u32) -> u32 {
    let inv = 255 - a8;
    let drb = d & 0x00FF_00FF;
    let dg = d & 0x0000_FF00;
    let orb = ((srb * a8 + drb * inv) >> 8) & 0x00FF_00FF;
    let og = ((sg * a8 + dg * inv) >> 8) & 0x0000_FF00;
    orb | og
}

// Span-fill an axis-aligned imgui quad [v0=(x0,y0) v1=(x1,y0) v2=(x1,y1)
// v3=(x0,y1)] with uniform vertex color. Returns false when the quad isn't
// actually axis-aligned/uniform (caller falls back to triangles). Covers
// solid rects (constant uv → single texture sample, opaque rows become
// slice::fill) and glyph quads (uv interpolated linearly, no barycentrics).
fn fill_axis_rect(
    dst: &mut [u32],
    dw: i32,
    clip: (i32, i32, i32, i32),
    atlas: (&[u8], usize, usize),
    v: [imgui::DrawVert; 4],
) -> bool {
    let [v0, v1, v2, v3] = v;
    let axis_aligned = v0.pos[1] == v1.pos[1]
        && v1.pos[0] == v2.pos[0]
        && v2.pos[1] == v3.pos[1]
        && v3.pos[0] == v0.pos[0];
    if !axis_aligned || v0.col != v1.col || v0.col != v2.col || v0.col != v3.col {
        return false;
    }
    let (x0f, y0f) = (v0.pos[0], v0.pos[1]);
    let (x1f, y1f) = (v2.pos[0], v2.pos[1]);
    if x1f <= x0f || y1f <= y0f {
        return true; // degenerate: nothing to draw, but handled
    }

    // Pixel centers (px+0.5) inside [x0f, x1f).
    let px0 = ((x0f - 0.5).ceil() as i32).max(clip.0);
    let py0 = ((y0f - 0.5).ceil() as i32).max(clip.1);
    let px1 = ((x1f - 0.5).ceil() as i32).min(clip.2);
    let py1 = ((y1f - 0.5).ceil() as i32).min(clip.3);
    if px0 >= px1 || py0 >= py1 {
        return true;
    }

    let srb = ((v0.col[0] as u32) << 16) | v0.col[2] as u32;
    let sg = (v0.col[1] as u32) << 8;
    let vcol_a = v0.col[3] as u32;

    let const_uv = v0.uv == v2.uv;
    if const_uv {
        // Solid fill — one texture sample (the atlas white texel).
        let a8 = vcol_a * sample_atlas(atlas, v0.uv[0], v0.uv[1]) / 255;
        if a8 == 0 {
            return true;
        }
        let solid = (srb & 0x00FF_0000) | sg | (srb & 0xFF);
        for py in py0..py1 {
            let row = (py * dw) as usize;
            if a8 >= 255 {
                dst[row + px0 as usize..row + px1 as usize].fill(solid);
            } else {
                for px in px0..px1 {
                    let i = row + px as usize;
                    dst[i] = blend_px(dst[i], srb, sg, a8);
                }
            }
        }
        return true;
    }

    // Textured quad (glyph): uv is linear across the rect — step it.
    let dudx = (v2.uv[0] - v0.uv[0]) / (x1f - x0f);
    let dvdy = (v2.uv[1] - v0.uv[1]) / (y1f - y0f);
    for py in py0..py1 {
        let row = (py * dw) as usize;
        let tv = v0.uv[1] + (py as f32 + 0.5 - y0f) * dvdy;
        let mut tu = v0.uv[0] + (px0 as f32 + 0.5 - x0f) * dudx;
        for px in px0..px1 {
            let tex_a = sample_atlas(atlas, tu, tv);
            tu += dudx;
            let a8 = vcol_a * tex_a / 255;
            if a8 > 0 {
                let i = row + px as usize;
                dst[i] = blend_px(dst[i], srb, sg, a8);
            }
        }
    }
    true
}

// General path: barycentric triangle fill with incrementally-stepped edge
// functions. Handles the leftovers — anti-aliased fringes, rounded-corner
// fans, lines. Detects constant color/uv so the common "solid convex fill"
// (e.g. the rounded window background) skips all per-pixel interpolation.
fn raster_tri(
    dst: &mut [u32],
    dw: i32,
    clip: (i32, i32, i32, i32),
    atlas: (&[u8], usize, usize),
    v0: imgui::DrawVert,
    v1: imgui::DrawVert,
    v2: imgui::DrawVert,
) {
    let (x0, y0) = (v0.pos[0], v0.pos[1]);
    let (x1, y1) = (v1.pos[0], v1.pos[1]);
    let (x2, y2) = (v2.pos[0], v2.pos[1]);

    let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
    if area.abs() < 1e-6 {
        return;
    }
    // Normalize winding so "inside" is all-weights >= 0.
    let flip = if area < 0.0 { -1.0 } else { 1.0 };
    let inv_area = flip / area;

    let min_x = x0.min(x1).min(x2).floor().max(clip.0 as f32) as i32;
    let min_y = y0.min(y1).min(y2).floor().max(clip.1 as f32) as i32;
    let max_x = x0.max(x1).max(x2).ceil().min(clip.2 as f32) as i32;
    let max_y = y0.max(y1).max(y2).ceil().min(clip.3 as f32) as i32;
    if min_x >= max_x || min_y >= max_y {
        return;
    }

    // Edge functions are linear in (x, y): evaluate at the top-left pixel
    // center once, then step by their x/y gradients.
    let fx0 = min_x as f32 + 0.5;
    let fy0 = min_y as f32 + 0.5;
    let e = |ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32| {
        ((ax - px) * (by - py) - (ay - py) * (bx - px)) * inv_area
    };
    let mut w0_row = e(x1, y1, x2, y2, fx0, fy0);
    let mut w1_row = e(x2, y2, x0, y0, fx0, fy0);
    let w0_dx = e(x1, y1, x2, y2, fx0 + 1.0, fy0) - w0_row;
    let w1_dx = e(x2, y2, x0, y0, fx0 + 1.0, fy0) - w1_row;
    let w0_dy = e(x1, y1, x2, y2, fx0, fy0 + 1.0) - w0_row;
    let w1_dy = e(x2, y2, x0, y0, fx0, fy0 + 1.0) - w1_row;

    let const_col = v0.col == v1.col && v0.col == v2.col;
    let const_uv = v0.uv == v1.uv && v0.uv == v2.uv;
    let const_a8 = if const_uv {
        sample_atlas(atlas, v0.uv[0], v0.uv[1])
    } else {
        255
    };
    let srb = ((v0.col[0] as u32) << 16) | v0.col[2] as u32;
    let sg = (v0.col[1] as u32) << 8;

    for py in min_y..max_y {
        let row = (py * dw) as usize;
        let mut w0 = w0_row;
        let mut w1 = w1_row;
        for px in min_x..max_x {
            let w2 = 1.0 - w0 - w1;
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                let tex_a = if const_uv {
                    const_a8
                } else {
                    let u = w0 * v0.uv[0] + w1 * v1.uv[0] + w2 * v2.uv[0];
                    let v = w0 * v0.uv[1] + w1 * v1.uv[1] + w2 * v2.uv[1];
                    sample_atlas(atlas, u, v)
                };
                if const_col {
                    let a8 = v0.col[3] as u32 * tex_a / 255;
                    if a8 > 0 {
                        let i = row + px as usize;
                        dst[i] = blend_px(dst[i], srb, sg, a8);
                    }
                } else {
                    // AA fringes interpolate color (mostly just alpha).
                    let r = (w0 * v0.col[0] as f32 + w1 * v1.col[0] as f32 + w2 * v2.col[0] as f32)
                        as u32;
                    let g = (w0 * v0.col[1] as f32 + w1 * v1.col[1] as f32 + w2 * v2.col[1] as f32)
                        as u32;
                    let b = (w0 * v0.col[2] as f32 + w1 * v1.col[2] as f32 + w2 * v2.col[2] as f32)
                        as u32;
                    let a = (w0 * v0.col[3] as f32 + w1 * v1.col[3] as f32 + w2 * v2.col[3] as f32)
                        as u32;
                    let a8 = a * tex_a / 255;
                    if a8 > 0 {
                        let i = row + px as usize;
                        dst[i] = blend_px(dst[i], (r << 16) | b, g << 8, a8);
                    }
                }
            }
            w0 += w0_dx;
            w1 += w1_dx;
        }
        w0_row += w0_dy;
        w1_row += w1_dy;
    }
}

// Nearest sample of the alpha8 font atlas → 0..=255. The atlas is baked at
// the exact DPI-scaled glyph size, so texels map 1:1 to screen pixels and
// filtering would only cost time.
#[inline(always)]
fn sample_atlas(atlas: (&[u8], usize, usize), u: f32, v: f32) -> u32 {
    let (data, aw, ah) = atlas;
    let x = ((u * aw as f32) as usize).min(aw - 1);
    let y = ((v * ah as f32) as usize).min(ah - 1);
    data[y * aw + x] as u32
}
