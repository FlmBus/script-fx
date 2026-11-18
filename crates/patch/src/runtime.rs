//! The sandboxed Luau runtime shared by the DSP side (`Patch::load`) and the UI
//! (`ui_components::Ui`): both evaluate the same `patch.luau`, which `require`s
//! its UI files.
//!
//! Sandbox: Luau's sandbox mode (read-only globals and builtins), no `loadstring`
//! / `getfenv` / `setfenv`, a memory cap, an interrupt budget per top-level call,
//! and a `require` that only loads files from inside the patch folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mlua::luau::{FsRequirer, NavigateError, Require};
use mlua::{Function, Lua, Table, Value, VmState};

use crate::{PatchError, invalid, locale};

/// Interrupts per top-level call. Luau interrupts at function calls and loop
/// back-edges. Measured peak of the factory patches: 28.5k (Junkyard's procedural
/// wood background); others < 2k. 500k ≈ 17× headroom and stops a runaway script
/// within a few tens of ms.
const INTERRUPT_BUDGET: u32 = 500_000;
const MEMORY_LIMIT: usize = 16 * 1024 * 1024;

const PATCH_PRELUDE: &str = include_str!("prelude.luau");
const UI_PRELUDE: &str = include_str!("ui_prelude.luau");

/// The patch's entry file.
pub const ENTRY: &str = "patch.luau";

/// A sandboxed Lua state for one patch folder.
pub struct Runtime {
    pub lua: Lua,
    budget: Arc<AtomicU32>,
    dir: PathBuf,
    export: Function,
}

impl Runtime {
    pub fn new(dir: &Path) -> Result<Self, PatchError> {
        let _c = locale::CNumeric::enter();
        let dir = dir.canonicalize()?;
        let lua = Lua::new();
        lua.set_memory_limit(MEMORY_LIMIT)?;
        let export: Function = lua.load(PATCH_PRELUDE).set_name("=prelude").eval()?;
        lua.load(UI_PRELUDE).set_name("=ui_prelude").exec()?;
        let globals = lua.globals();
        for name in ["loadstring", "getfenv", "setfenv"] {
            globals.set(name, Value::Nil)?;
        }
        globals.set("require", lua.create_require_function(PatchRequirer::new(dir.clone()))?)?;
        // From here on globals and builtins are read-only for patch code.
        lua.sandbox(true)?;

        let budget = Arc::new(AtomicU32::new(0));
        let counter = budget.clone();
        lua.set_interrupt(move |_| {
            if counter.fetch_add(1, Ordering::Relaxed) > INTERRUPT_BUDGET {
                Err(mlua::Error::runtime("instruction budget exceeded"))
            } else {
                Ok(VmState::Continue)
            }
        });
        Ok(Runtime { lua, budget, dir, export })
    }

    /// Starts a new top-level call: resets the instruction budget.
    pub fn begin_call(&self) {
        self.budget.store(0, Ordering::Relaxed);
    }

    /// Evaluates `patch.luau` (fresh: the `require` cache is cleared first, so
    /// edited UI files are picked up) and returns the `Patch` object it returns.
    pub fn eval_patch(&self) -> Result<Table, PatchError> {
        let _c = locale::CNumeric::enter();
        if let Ok(cache) = self.lua.named_registry_value::<Table>("__MLUA_LOADER_CACHE") {
            cache.clear()?;
        }
        let file = self.dir.join(ENTRY);
        let src = std::fs::read_to_string(&file)?;
        self.begin_call();
        let result: Value = self.lua.load(src).set_name(format!("@{}", file.display())).eval()?;
        match result {
            Value::Table(t) if t.get::<Option<bool>>("__is_patch")?.unwrap_or(false) => Ok(t),
            _ => Err(invalid(format!("{ENTRY} must end with `return Patch.new(...)`"))),
        }
    }

    /// Turns a `Patch` object into plain data (see the end of `prelude.luau`).
    pub fn export(&self, patch: &Table) -> Result<Table, PatchError> {
        let _c = locale::CNumeric::enter();
        self.begin_call();
        Ok(self.export.call(patch.clone())?)
    }
}

/// Luau's standard `require-by-string` (relative to the requiring file), but
/// modules may only be loaded from inside the patch folder, and `.luaurc` files
/// on disk are ignored.
struct PatchRequirer {
    inner: FsRequirer,
    root: PathBuf,
}

impl PatchRequirer {
    fn new(root: PathBuf) -> Self {
        PatchRequirer { inner: FsRequirer::new(), root }
    }
}

type Nav = Result<(), NavigateError>;

impl Require for PatchRequirer {
    fn is_require_allowed(&self, chunk: &str) -> bool {
        self.inner.is_require_allowed(chunk)
    }
    fn reset(&mut self, chunk: &str) -> Nav {
        self.inner.reset(chunk)
    }
    fn jump_to_alias(&mut self, path: &str) -> Nav {
        self.inner.jump_to_alias(path)
    }
    fn to_parent(&mut self) -> Nav {
        self.inner.to_parent()
    }
    fn to_child(&mut self, name: &str) -> Nav {
        self.inner.to_child(name)
    }
    fn has_module(&self) -> bool {
        self.inner.has_module()
    }
    fn cache_key(&self) -> String {
        self.inner.cache_key()
    }
    fn has_config(&self) -> bool {
        false
    }
    fn config(&self) -> std::io::Result<Vec<u8>> {
        Err(std::io::ErrorKind::NotFound.into())
    }
    fn loader(&self, lua: &Lua) -> mlua::Result<Function> {
        let path = Path::new(&self.inner.cache_key()).canonicalize().map_err(mlua::Error::external)?;
        if !path.starts_with(&self.root) {
            return Err(mlua::Error::runtime("require: only files inside the patch folder can be loaded"));
        }
        // Named by its real file path, so errors read ".../ui.luau:12: ..." and
        // relative requires inside it resolve from its folder.
        lua.load(path.as_path()).set_name(format!("@{}", path.display())).into_function()
    }
}
