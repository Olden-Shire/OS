// @ObfuscatedName("fu") — jag::oldscape::dash3d::ModelSource (abstract).
//
// Java's abstract base for anything that can be rendered into the 3D
// scene: ModelLit overrides worldRender directly; everything else
// (ClientLocAnim, ClientNpc, ClientPlayer, ClientProj, MapSpotAnim)
// implements getTempModel() and inherits the default worldRender that
// composes the temp model and forwards.
//
// Rust shape: a single struct with an interior-mutable kind so
// World.shareLight can swap an Unlit model for its Lit result in
// place (Java mutates the `Wall.modelA` field the same way).
//
//   Lit    — a fully-lit static model (Java: ModelLit stored directly)
//   Unlit  — a pre-shareLight ModelUnlit (Java stores ModelUnlit in
//            the same field during ClientBuild; World.shareLight pairs
//            + lights them). ModelUnlit.getTempModel() returns null in
//            Java so an Unlit that survives to render time draws
//            nothing — same here.
//   Temp   — Java's getTempModel dispatch: a closure that composes the
//            current-frame model (animated loc / entity). worldRender
//            refreshes `min_y` from the composed model, mirroring
//            `this.minY = model.minY` in ModelSource.worldRender.

#![allow(dead_code)]

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use crate::dash3d::model_lit::ModelLit;
use crate::dash3d::model_unlit::ModelUnlit;

pub type TempModelFn = dyn Fn() -> Option<Arc<ModelLit>> + Send + Sync;

pub enum ModelSourceKind {
    Lit(Arc<ModelLit>),
    Unlit(ModelUnlit),
    Temp(Arc<TempModelFn>),
}

pub struct ModelSource {
    pub kind: Mutex<ModelSourceKind>,
    // @ObfuscatedName("fu.n") — minY, default 1000; refreshed from the
    // temp model after each worldRender.
    pub min_y: AtomicI32,
}

impl ModelSource {
    pub fn lit(m: Arc<ModelLit>) -> Arc<Self> {
        // ModelLit IS-A ModelSource in Java, so its own minY is live
        // immediately (the Temp path only refreshes at render time).
        let min_y = m.min_y;
        Arc::new(Self {
            kind: Mutex::new(ModelSourceKind::Lit(m)),
            min_y: AtomicI32::new(min_y),
        })
    }

    pub fn unlit(m: ModelUnlit) -> Arc<Self> {
        Arc::new(Self {
            kind: Mutex::new(ModelSourceKind::Unlit(m)),
            min_y: AtomicI32::new(1000),
        })
    }

    pub fn temp(f: Arc<TempModelFn>) -> Arc<Self> {
        Arc::new(Self {
            kind: Mutex::new(ModelSourceKind::Temp(f)),
            min_y: AtomicI32::new(1000),
        })
    }

    // @ObfuscatedName("fu.n") read — Java field access `model.minY`.
    pub fn min_y(&self) -> i32 {
        self.min_y.load(Ordering::Relaxed)
    }

    // @ObfuscatedName("fu.z(IIIIIIIII)V") — ModelSource.worldRender.
    // Verbatim port of ModelSource.java:14-20 with the ModelLit
    // override (ModelLit.java:915) dispatched through the Lit arm.
    pub fn world_render(
        &self,
        yaw: i32,
        sin_pitch: i32, cos_pitch: i32,
        sin_yaw: i32, cos_yaw: i32,
        rel_x: i32, rel_y: i32, rel_z: i32,
        typecode: i32,
    ) {
        // Clone the renderable out so the lock isn't held across the
        // raster calls (Temp closures may need to lock other state).
        let model: Option<Arc<ModelLit>> = {
            let kind = self.kind.lock().unwrap();
            match &*kind {
                ModelSourceKind::Lit(m) => Some(Arc::clone(m)),
                // Java: ModelUnlit.getTempModel() returns null →
                // default worldRender draws nothing.
                ModelSourceKind::Unlit(_) => None,
                ModelSourceKind::Temp(f) => f(),
            }
        };
        if let Some(m) = model {
            self.min_y.store(m.min_y, Ordering::Relaxed);
            m.world_render(yaw, sin_pitch, cos_pitch, sin_yaw, cos_yaw,
                           rel_x, rel_y, rel_z, typecode);
        }
    }

    // World.setObj helper — Java's `instanceof ModelLit` +
    // calcBoundingCylinder + minY read. Returns None for non-Lit
    // sources; computes the cylinder minY fresh when the model's
    // bounding cache isn't populated (we can't mutate through the Arc).
    pub fn lit_bounded_min_y(&self) -> Option<i32> {
        let kind = self.kind.lock().unwrap();
        let ModelSourceKind::Lit(m) = &*kind else { return None };
        if m.bounding_calc == 1 {
            return Some(m.min_y);
        }
        let mut min_y = 0i32;
        for i in 0..m.num_points as usize {
            if -m.point_y[i] > min_y {
                min_y = -m.point_y[i];
            }
        }
        Some(min_y)
    }

    // World.shareLight downcast helper — Java's
    // `instanceof ModelUnlit` checks. Runs `f` with the contained
    // ModelUnlit if this source is still unlit; returns None otherwise.
    pub fn with_unlit_mut<R>(&self, f: impl FnOnce(&mut ModelUnlit) -> R) -> Option<R> {
        let mut kind = self.kind.lock().unwrap();
        match &mut *kind {
            ModelSourceKind::Unlit(u) => Some(f(u)),
            _ => None,
        }
    }

    pub fn is_unlit(&self) -> bool {
        matches!(&*self.kind.lock().unwrap(), ModelSourceKind::Unlit(_))
    }

    // World.shareLight conversion — Java's
    // `var8.modelA = var9.light(var9.ambient, var9.contrast, x, y, z)`.
    // Lights the contained ModelUnlit in place and swaps this source
    // to the Lit result.
    pub fn light_in_place(&self, light_x: i32, light_y: i32, light_z: i32) {
        let mut kind = self.kind.lock().unwrap();
        if let ModelSourceKind::Unlit(u) = &mut *kind {
            let ambient = u.ambient as i32;
            let contrast = u.contrast as i32;
            let lit = ModelLit::light(u, ambient, contrast, light_x, light_y, light_z);
            let min_y = lit.min_y;
            *kind = ModelSourceKind::Lit(Arc::new(lit));
            self.min_y.store(min_y, Ordering::Relaxed);
        }
    }
}

// Pair two ModelSources for the shareLight normal-summing pass. Only
// does work when BOTH are still Unlit (Java's double instanceof).
// Mirrors `ModelUnlit.shareLight(a, b, x, y, z, hideOccludedFaces)`.
pub fn share_light_pair(a: &ModelSource, b: &ModelSource,
                        off_x: i32, off_y: i32, off_z: i32,
                        mark_for_type_2: bool) {
    // A multi-tile sprite is reachable from several squares, so the
    // neighbour walk can hand us the same ModelSource on both sides —
    // Java tolerates the aliased call; for us it would deadlock on the
    // second lock. Self-pairing adds nothing (offset 0 sums a vertex
    // with itself), so skip it.
    if std::ptr::eq(a, b) {
        return;
    }
    // Lock ordering: both kinds locked here; callers never hold either
    // lock when calling (single-threaded build pass).
    let mut ka = a.kind.lock().unwrap();
    let mut kb = b.kind.lock().unwrap();
    if let (ModelSourceKind::Unlit(ua), ModelSourceKind::Unlit(ub)) = (&mut *ka, &mut *kb) {
        ModelUnlit::share_light(ua, ub, off_x, off_y, off_z, mark_for_type_2);
    }
}
