//! `cargo xtask bundle scriptfx --release` → target/bundled/ScriptFX.{vst3,clap}
//! `cargo xtask bundle-universal scriptfx --release` → macOS universal (arm64 + x86_64)
fn main() -> nice_plug_xtask::Result<()> {
    nice_plug_xtask::main()
}
