//! Software renderer — direct port of the rev1 Java client's `jagex3.graphics` + `dash3d`
//! packages. Pix2D owns a pixel buffer; Pix3D rasterizes triangles into it; sprite +
//! font blitters draw on top. Together this is what `Client.drawInterface` calls.
//!
//! ## Java → Rust port notes
//!
//! Java's classes use `static` fields for the current pixel buffer, clip rect, etc. —
//! they're effectively a thread-local rendering context. We model this as a `Pix2D`
//! STRUCT that owns its buffer + clip + dimensions. Higher-level renderers
//! (`Pix3D`, sprite/font blitters) take `&mut Pix2D` rather than reading static fields.
//!
//! ARGB pixel format: `0xAARRGGBB` packed into `u32`. Java uses `int` (i32) but the
//! bitwise behaviour is identical with `as u32` casts. Alpha is treated as opaque (full)
//! except where explicit per-pixel transparency math is involved.

pub mod font;
pub mod model_lit;
pub mod model_render;
pub mod pix2d;
pub mod pix3d;
pub mod sprite;

pub use font::{parse_tagged, PixFont, StyledRun};
pub use model_lit::{light as model_light, LitModel};
pub use model_render::ModelRenderer;
pub use pix2d::Pix2D;
pub use pix3d::Pix3D;
pub use sprite::{Pix32, Pix8, sheet_to_pix32, sheet_to_pix8};
