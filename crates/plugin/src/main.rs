use nice_plug::prelude::*;

fn main() {
    // Only the standalone may end itself on `SCRIPTFX_DIAG_EXIT_AFTER` (`diag.rs`): inside a host
    // that would take the host down with us.
    scriptfx::diag::allow_exit();
    nice_export_standalone::<scriptfx::ScriptFx>();
}
