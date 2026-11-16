# Local patches to nih-plug

Vendored from https://github.com/robbert-vdh/nih-plug at `de421011` (ISC).
Only `nih_plug` (root), `nih_plug_derive`, `nih_plug_egui` and `nih_plug_xtask` were copied;
the upstream `[workspace]` and `[profile]` sections were removed from `Cargo.toml`.
All changes are marked `PATCH(scriptfx)`.

Licensing note: the VST3 export goes through `vst3-sys`, which is GPLv3. Binaries that
export VST3 are therefore GPLv3 unless that dependency is replaced.

## 1. Runtime-changeable parameter names

Hosts key automation by parameter ID, so renaming a parameter is safe; nih-plug just
had no way to do it.

- `Param::dynamic_name()` / `Param::display_name()` (default: static name)
- `FloatParam::with_dynamic_name(Arc<RwLock<String>>)`: empty string falls back to the static name
- `ParamPtr::display_name()`
- VST3 `getParameterInfo` and CLAP `params_get_info` report `display_name()`
- `GuiContext::rescan_param_info()` and `InitContext::rescan_param_info()` (default no-op, e.g. standalone)
  - VST3: `restartComponent(kParamTitlesChanged)` (via the GUI-thread task queue)
  - CLAP: `host_params.rescan(CLAP_PARAM_RESCAN_INFO | CLAP_PARAM_RESCAN_TEXT)` (new `Task::RescanParamInfo`)

Whether a host actually re-reads names is host-specific: test per DAW.
