//! The viewport: the window's content area, which shows exactly one **surface** at a time.
//!
//! There is no tier between the viewport and its surfaces: the chain and the Matrix are surfaces
//! in exactly the sense a patch's own UI is (`docs/GLOSSARY.md`). The shell holds a [`SurfaceId`]
//! and the notch switcher lists what [`surfaces`] returns, so a patch declaring more than one of
//! its own just grows the list.
//!
//! What used to be per-view special cases in the shell lives here as per-surface traits
//! ([`SurfaceId::owns_block_modal`], [`SurfaceId::shows_patch_ui`], [`SurfaceId::DEFAULT`]).
//!
//! Two different things, kept apart on purpose:
//!
//! - **Which surfaces exist** is a list, built per patch: [`surfaces`]. That is where a new
//!   surface is registered, and where a patch's own 0..n surfaces come from — the switcher shows
//!   exactly what it returns, so nothing else has to know how many there are.
//! - **What a kind of surface behaves like** is a question asked of a [`SurfaceId`], from places
//!   that hold nothing but the id (the frame tick, the shell's dispatch). Every one of these is an
//!   **exhaustive match, never `matches!`**: adding a surface is then a compile error in each
//!   place that has to decide something about it, instead of silently getting the `false` answer.
//!   `Shell::window`'s surface match works the same way.

use patch::Patch;

use super::icons;

/// Which surface the viewport shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceId {
    /// One of the patch's own surfaces, by index; the shell draws its default macro surface
    /// instead when the patch declares none. Only index 0 exists today.
    Patch(usize),
    /// The chain: the audio tree (blocks, containers with slots). Owns the block modal.
    Chain,
    /// The Matrix: every modulation route as a table.
    Matrix,
}

impl SurfaceId {
    /// Where a patch load and leaving passive mode land: the patch's first surface.
    pub const DEFAULT: Self = Self::Patch(0);

    /// Does this surface own the block modal (and its camera)? Only the chain has blocks to zoom
    /// into, so switching away from it closes the modal.
    pub fn owns_block_modal(self) -> bool {
        match self {
            Self::Chain => true,
            Self::Patch(_) | Self::Matrix => false,
        }
    }

    /// Does this surface draw the patch's own UI — its components, rasterised into layers?
    ///
    /// The shell keeps those layers up to date only while the answer is yes: re-rendering them is
    /// the most expensive thing the editor does (and every changed layer is a texture upload too),
    /// and on the chain or the Matrix none of it is visible. Coming back re-renders and re-uploads
    /// on the next frame — a one-off, like opening the window, instead of a cost on every frame
    /// spent elsewhere. Keep this in step with the surface match in `Shell::window`, which decides
    /// what is actually drawn.
    pub fn shows_patch_ui(self) -> bool {
        match self {
            // Any of the patch's own surfaces, whichever index: the switcher lists what
            // [`surfaces`] returns, so a patch with several of them needs nothing here.
            Self::Patch(_) => true,
            Self::Chain | Self::Matrix => false,
        }
    }

    /// `SCRIPTFX_DEV_SURFACE=chain|matrix|surface` starts on that surface.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "surface" | "patch" => Some(Self::DEFAULT),
            "chain" => Some(Self::Chain),
            "matrix" => Some(Self::Matrix),
            _ => None,
        }
    }
}

/// One surface in the switcher. `Copy` with `&'static` text, so a built segment borrows nothing
/// from the list.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub id: SurfaceId,
    /// Shown as the icon's tooltip.
    pub label: &'static str,
    pub icon: &'static str,
}

/// The surfaces available for `patch`, in switcher order: its own first, then the built-ins.
///
/// A patch without a surface of its own still gets the first entry — the shell's default macro
/// surface stands in for it, so the switcher never changes shape.
pub fn surfaces(_patch: &Patch) -> Vec<Entry> {
    vec![
        Entry { id: SurfaceId::DEFAULT, label: "Surface", icon: icons::VIEW_SURFACE },
        Entry { id: SurfaceId::Chain, label: "Chain", icon: icons::VIEW_CHAIN },
        Entry { id: SurfaceId::Matrix, label: "Matrix", icon: icons::VIEW_MATRIX },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shell skips rendering the patch's components when this says no, so a surface that
    /// draws them must answer yes — see `Shell::window`.
    #[test]
    fn only_patch_surfaces_show_the_patch_ui() {
        assert!(SurfaceId::DEFAULT.shows_patch_ui());
        assert!(SurfaceId::Patch(1).shows_patch_ui());
        assert!(!SurfaceId::Chain.shows_patch_ui());
        assert!(!SurfaceId::Matrix.shows_patch_ui());
    }

    /// Only the chain has blocks to zoom into.
    #[test]
    fn only_the_chain_owns_the_block_modal() {
        assert!(SurfaceId::Chain.owns_block_modal());
        assert!(!SurfaceId::DEFAULT.owns_block_modal());
        assert!(!SurfaceId::Matrix.owns_block_modal());
    }
}
