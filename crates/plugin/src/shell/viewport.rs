//! The viewport: the window's content area, which shows exactly one **surface** at a time.
//!
//! There is no tier between the viewport and its surfaces: the chain and the Matrix are surfaces
//! in exactly the sense a patch's own UI is (`docs/GLOSSARY.md`). The shell holds a [`SurfaceId`]
//! and the notch switcher lists what [`surfaces`] returns, so a patch declaring more than one of
//! its own just grows the list.
//!
//! What used to be per-view special cases in the shell lives here as per-surface traits
//! ([`SurfaceId::wants_drawer`], [`SurfaceId::owns_block_modal`], [`SurfaceId::DEFAULT`]).

use patch::Patch;

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

    /// Does the modulator drawer belong over this surface? The Matrix already *is* every route,
    /// so the drawer would only duplicate it (and it closes when the Matrix opens).
    pub fn wants_drawer(self) -> bool {
        !matches!(self, Self::Matrix)
    }

    /// Does this surface own the block modal (and its camera)? Only the chain has blocks to zoom
    /// into, so switching away from it closes the modal.
    pub fn owns_block_modal(self) -> bool {
        matches!(self, Self::Chain)
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

/// One surface in the notch's switcher. `Copy`, so a built segment borrows only the label,
/// not the list.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub id: SurfaceId,
    pub label: &'a str,
}

/// The surfaces available for `patch`, in switcher order: its own first, then the built-ins.
///
/// A patch without a surface of its own still gets the first entry — the shell's default macro
/// surface stands in for it, so the switcher never changes shape.
pub fn surfaces(_patch: &Patch) -> Vec<Entry<'_>> {
    vec![
        Entry { id: SurfaceId::DEFAULT, label: "Surface" },
        Entry { id: SurfaceId::Chain, label: "Chain" },
        Entry { id: SurfaceId::Matrix, label: "Matrix" },
    ]
}
