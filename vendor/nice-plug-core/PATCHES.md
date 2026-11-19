# Local patches to nice-plug (+ nice-plug-core)

Vendored from crates.io: `nice-plug` 0.4.2 (`vendor/nice-plug`) and `nice-plug-core` 0.4.2
(`vendor/nice-plug-core`), ISC. Upstream: https://codeberg.org/RustAudio/nice-plug (the community
continuation of nih-plug). Applied via `[patch.crates-io]` in the root `Cargo.toml`. All changes are
marked `PATCH(scriptfx)`. Ported from our former nih-plug patch (same behaviour).

## 1. Runtime-changeable parameter names

Hosts key automation by parameter ID, so renaming a parameter is safe; nice-plug has no way to do it.

- core `Param::dynamic_name()` / `Param::display_name()` (default: static name), `ParamPtr::display_name()`
- core `FloatParam::with_dynamic_name(Arc<RwLock<String>>)`: empty string falls back to the static name
- core `GuiContext::rescan_param_info()`, `GuiContextInner::rescan_param_info()` and
  `ActivateContext::rescan_param_info()` (default no-op, e.g. standalone)
- VST3 `getParameterInfo` and CLAP `params_get_info` report `display_name()`
- VST3: `restartComponent(kParamTitlesChanged)` via the GUI task queue;
  CLAP: `host_params.rescan(RESCAN_INFO | RESCAN_TEXT)` (new `Task::RescanParamInfo`)

Worth upstreaming. Whether a host re-reads names is host-specific: test per DAW.
