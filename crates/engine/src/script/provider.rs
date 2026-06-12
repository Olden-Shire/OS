//! Script registry — mirrors the Engine-TS reference
//! ScriptProvider.ts: loads server/script.{dat,idx}, indexes by id,
//! name, and the packed trigger lookup key.

use std::collections::HashMap;
use std::sync::Arc;

use io::packet::Packet;

use crate::script::file::ScriptFile;
use crate::script::trigger::Trigger;

/// The compiler version the runtime expects in script.dat's header.
/// v27: per-script lookup keys widened to i64 so component subjects
/// (packed (interface<<16)|child, e.g. [if_button,if_378:6]) fit
/// after the <<10 shift.
pub const COMPILER_VERSION: i32 = 27;

#[derive(Default)]
pub struct ScriptProvider {
    scripts: Vec<Option<Arc<ScriptFile>>>,
    names: HashMap<String, i32>,
    lookup: HashMap<i64, Arc<ScriptFile>>,
}

impl ScriptProvider {
    /// Load `dir/server/script.dat` + `.idx`. Returns scripts loaded.
    pub fn load(dir: &str) -> Result<ScriptProvider, String> {
        let dat = std::fs::read(format!("{dir}/server/script.dat"))
            .map_err(|e| format!("script.dat: {e}"))?;
        let idx = std::fs::read(format!("{dir}/server/script.idx"))
            .map_err(|e| format!("script.idx: {e}"))?;
        Self::parse(dat, idx)
    }

    pub fn parse(dat: Vec<u8>, idx: Vec<u8>) -> Result<ScriptProvider, String> {
        let mut dat = Packet::from_vec(dat);
        let mut idx = Packet::from_vec(idx);

        let entries = dat.g4();
        idx.pos += 4;

        let version = dat.g4();
        if version != COMPILER_VERSION {
            return Err(format!(
                "scripts compiled with incompatible compiler (got {version}, want {COMPILER_VERSION})"
            ));
        }

        let mut provider = ScriptProvider {
            scripts: (0..entries).map(|_| None).collect(),
            names: HashMap::new(),
            lookup: HashMap::new(),
        };

        let mut loaded = 0;
        for id in 0..entries {
            let size = idx.g4();
            if size == 0 {
                continue;
            }

            let mut blob = vec![0u8; size as usize];
            dat.gdata(&mut blob, 0, size as usize);

            match ScriptFile::decode(id, blob) {
                Ok(script) => {
                    let script = Arc::new(script);
                    provider.names.insert(script.info.script_name.clone(), id);
                    // -1 lookup keys are non-triggerable (procs/labels
                    // resolve by id/name instead).
                    if script.info.lookup_key != -1 {
                        provider.lookup.insert(
                            script.info.lookup_key,
                            Arc::clone(&script),
                        );
                    }
                    provider.scripts[id as usize] = Some(script);
                    loaded += 1;
                }
                Err(err) => {
                    eprintln!("[scripts] failed to load script {id}: {err}");
                    return Err(format!("script {id}: {err}"));
                }
            }
        }

        eprintln!("[scripts] loaded {loaded} scripts");
        Ok(provider)
    }

    pub fn get(&self, id: i32) -> Option<Arc<ScriptFile>> {
        self.scripts.get(id as usize).and_then(|o| o.clone())
    }

    pub fn get_by_name(&self, name: &str) -> Option<Arc<ScriptFile>> {
        self.names.get(name).and_then(|&id| self.get(id))
    }

    /// Trigger lookup with the reference's type → category → global
    /// fall-through.
    pub fn get_by_trigger(&self, trigger: Trigger, type_id: i32, category: i32)
        -> Option<Arc<ScriptFile>>
    {
        let t = trigger as i64;
        if type_id != -1 {
            if let Some(s) = self.lookup.get(&(t | (0x2 << 8) | ((type_id as i64) << 10))) {
                return Some(Arc::clone(s));
            }
        }
        if category != -1 {
            if let Some(s) = self.lookup.get(&(t | (0x1 << 8) | ((category as i64) << 10))) {
                return Some(Arc::clone(s));
            }
        }
        self.lookup.get(&t).map(Arc::clone)
    }

    /// Exact-combo lookup (no fall-through).
    pub fn get_by_trigger_specific(&self, trigger: Trigger, type_id: i32, category: i32)
        -> Option<Arc<ScriptFile>>
    {
        let t = trigger as i64;
        if type_id != -1 {
            return self.lookup.get(&(t | (0x2 << 8) | ((type_id as i64) << 10))).map(Arc::clone);
        }
        if category != -1 {
            return self.lookup.get(&(t | (0x1 << 8) | ((category as i64) << 10))).map(Arc::clone);
        }
        self.lookup.get(&t).map(Arc::clone)
    }
}

#[cfg(test)]
impl ScriptProvider {
    /// Empty provider for tests.
    pub fn test_empty() -> Self {
        ScriptProvider {
            scripts: Vec::new(),
            names: HashMap::new(),
            lookup: HashMap::new(),
        }
    }

    /// Register `script` for the specific (trigger, type_id) combo — the same
    /// key `get_by_trigger_specific` / `get_by_trigger` look up.
    pub fn test_insert_specific(&mut self, trigger: Trigger, type_id: i32, script: Arc<ScriptFile>) {
        let t = trigger as i64;
        self.lookup.insert(t | (0x2 << 8) | ((type_id as i64) << 10), script);
    }
}

#[cfg(test)]
impl ScriptProvider {
    /// Register `script` for the trigger with no type/category (the plain key,
    /// e.g. [logout,_] / [login,_]).
    pub fn test_insert_global(&mut self, trigger: Trigger, script: Arc<ScriptFile>) {
        self.lookup.insert(trigger as i64, script);
    }

    /// Register `script` under its id so `get(id)` resolves it (the lookup
    /// SETTIMER / GETTIMER / gosub-by-id use).
    pub fn test_insert_by_id(&mut self, id: i32, script: Arc<ScriptFile>) {
        if self.scripts.len() <= id as usize {
            self.scripts.resize(id as usize + 1, None);
        }
        self.scripts[id as usize] = Some(script);
    }
}
