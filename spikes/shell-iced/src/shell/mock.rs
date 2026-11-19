//! Mock patches, the four from the mockup. In the real shell this comes from the patch
//! library (scan of the patches folder) and the loaded patch's chain.

use super::theme::font;
use nice_plug_iced::iced::{Color, Font, color, font::Weight};

pub struct Patch {
    pub id: &'static str,
    pub name: &'static str,
    pub snapshots: &'static [&'static str],
    /// The patch's own colours and type (the library borrows them).
    pub accent: Color,
    pub ink: Color,
    pub title_color: Color,
    pub title_font: Font,
    pub title_upper: bool,
    /// Background art, approximated as a two-stop linear gradient (top-left → bottom-right).
    pub art: (Color, Color),
    /// Shader tint for the stage stand-in art.
    pub glow: (Color, Color),
    pub chain: &'static [Link],
}

#[derive(Clone, Copy)]
pub enum Led {
    Ok,
    Reduce,
    Clip,
}

pub enum Link {
    Node(&'static str, Led),
    /// Parallel lanes (e.g. a band split), each a list of nodes.
    Split(&'static [&'static [(&'static str, Led)]]),
}

const fn named(name: &'static str, weight: Weight, italic: bool) -> Font {
    Font {
        family: nice_plug_iced::iced::font::Family::Name(name),
        weight,
        stretch: nice_plug_iced::iced::font::Stretch::Normal,
        style: if italic { nice_plug_iced::iced::font::Style::Italic } else { nice_plug_iced::iced::font::Style::Normal },
    }
}

use Led::*;
use Link::*;

pub const PATCHES: &[Patch] = &[
    Patch {
        id: "neon",
        name: "Neon Saturator",
        snapshots: &["Init Warmth", "Crunch", "Bus glue"],
        accent: color!(0xff6fbf),
        ink: color!(0xf2e9ff),
        title_color: color!(0xffa0c8),
        title_font: named("Orbitron", Weight::ExtraBold, false),
        title_upper: true,
        art: (color!(0x5a1f8a), color!(0x0d0820)),
        glow: (color!(0xd63cff), color!(0x2878ff)),
        chain: &[Node("tape", Ok), Node("tube", Ok), Node("diode", Clip), Node("soft-clip", Reduce), Node("auto-gain", Ok)],
    },
    Patch {
        id: "bass",
        name: "Bass Helper",
        snapshots: &["Init", "Chorus push", "Slap"],
        accent: color!(0xf2c14e),
        ink: color!(0xe8edf2),
        title_color: color!(0xf2c14e),
        title_font: named("Inter", Weight::Bold, false),
        title_upper: true,
        art: (color!(0x1f2a33), color!(0x10151a)),
        glow: (color!(0xf2c14e), color!(0x2a5a7a)),
        chain: &[
            Node("punch", Ok),
            Node("fet", Reduce),
            Node("opto", Reduce),
            Split(&[&[("rider", Ok)], &[("grit", Ok), ("fizz", Ok)]]),
            Node("eq", Ok),
            Node("out", Ok),
        ],
    },
    Patch {
        id: "tape",
        name: "Tape Glow",
        snapshots: &["Init", "Lo-fi bus"],
        accent: color!(0xff8a3d),
        ink: color!(0xf3e6d8),
        title_color: color!(0xffb07a),
        title_font: named("Playfair Display", Weight::Semibold, true),
        title_upper: false,
        art: (color!(0x3d2620), color!(0x17141c)),
        glow: (color!(0xff8a3d), color!(0x5a2a6a)),
        chain: &[Node("tape", Clip), Node("tone", Ok), Node("output", Ok)],
    },
    Patch {
        id: "clean",
        name: "Clean Gain",
        snapshots: &["Init"],
        accent: color!(0x4fd1c5),
        ink: color!(0xe6f4f1),
        title_color: color!(0x4fd1c5),
        title_font: font::MONO,
        title_upper: false,
        art: (color!(0x16292e), color!(0x0c1517)),
        glow: (color!(0x4fd1c5), color!(0x123a44)),
        chain: &[Node("gain", Ok)],
    },
];
