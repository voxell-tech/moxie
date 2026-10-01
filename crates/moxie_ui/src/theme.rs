//! Editor theme: the raw Monokai Pro palette plus the semantic slots
//! the UI reads, grouped by what they govern.
//!
//! Palette mirrors
//! `examples/bevy_examples/assets/typst/monokai_pro.typ`
//! so typst-rendered content and the editor chrome share one look.

use std::time::Duration;

use bevy::prelude::*;
use bevy_fynix::tokens::{
    Curve, Motion as MotionKind, MotionTokens, SpacingTokens,
    SurfaceTokens, TextTokens, Tone,
};
use bevy_motiongfx::motiongfx::motiongfx_interp::ease::{
    self, EaseFn,
};

/// The editor's ground colour, also [`Colors::bg`].
pub const BG: Color = Color::srgb_u8(0x19, 0x18, 0x1A);

/// Raw Monokai Pro palette.
#[derive(Clone, Debug)]
pub struct Palette {
    pub red: Color,
    pub orange: Color,
    pub yellow: Color,
    pub green: Color,
    pub blue: Color,
    pub purple: Color,
    /// Darkest → lightest neutrals.
    pub base: [Color; 9],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            red: Color::srgb_u8(0xFF, 0x61, 0x88),
            orange: Color::srgb_u8(0xFC, 0x98, 0x67),
            yellow: Color::srgb_u8(0xFF, 0xD8, 0x66),
            green: Color::srgb_u8(0xA9, 0xDC, 0x76),
            blue: Color::srgb_u8(0x78, 0xDC, 0xE8),
            purple: Color::srgb_u8(0xAB, 0x9D, 0xF2),
            base: [
                BG,
                Color::srgb_u8(0x22, 0x1F, 0x22),
                Color::srgb_u8(0x2D, 0x2A, 0x2E),
                Color::srgb_u8(0x40, 0x3E, 0x41),
                Color::srgb_u8(0x5B, 0x59, 0x5C),
                Color::srgb_u8(0x72, 0x70, 0x72),
                Color::srgb_u8(0x93, 0x92, 0x93),
                Color::srgb_u8(0xC1, 0xC0, 0xC0),
                Color::srgb_u8(0xFC, 0xFC, 0xFA),
            ],
        }
    }
}

/// The editor's look, grouped by what it governs.
#[derive(Clone, Debug)]
pub struct EditorTheme {
    pub palette: Palette,
    pub color: Colors,
    pub space: Spacing,
    pub text: TextScale,
    pub motion: Motion,
    pub layer: Layers,
}

/// Semantic colour slots. A fill is translucent and layers over
/// whatever is behind it, a ground is opaque.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    /// Primary (active) text.
    pub text: Color,
    /// Secondary / inactive text and icons.
    pub text_dim: Color,
    /// A third, fainter tier, for a de-emphasised label beside a
    /// brighter one.
    pub text_faint: Color,
    /// Interactive accent.
    pub accent: Color,
    /// Destructive accents, and a draft or error state.
    pub critical: Color,
    /// The editor's own ground.
    pub bg: Color,
    /// Panels and popups.
    pub panel: Color,
    /// A filled control's resting surface.
    pub fill: Color,
    /// A barely-there fill, for a tint rather than a surface.
    pub fill_faint: Color,
    /// Dividers and borders.
    pub hairline: Color,
    /// The overlay a plain surface fades to under the cursor.
    pub hover: Color,
    /// A selected row's surface tint.
    pub selection: Color,
    /// A timeline clip's fill.
    pub clip: Color,
    /// A clip's own hover and press brighten, in its blue family
    /// rather than [`Self::hover`]'s neutral gray.
    pub clip_hover: Color,
    pub clip_press: Color,
}

/// The spacing and sizing scale.
#[derive(Clone, Copy, Debug)]
pub struct Spacing {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
    /// The default corner.
    pub radius: f32,
    /// The standard height of a row or an interactive control.
    pub row: f32,
    /// A timeline action's height.
    pub action_row: f32,
    /// The vertical gap between timeline rows that would otherwise
    /// overlap in time.
    pub lane_gap: f32,
    /// A toolbar button's square.
    pub touch: f32,
    /// The default icon.
    pub icon: f32,
    /// A divider or rail's thickness.
    pub hairline: f32,
    /// A drawn edge that has to read as deliberate: a drop insertion
    /// line, a drop target's outline.
    pub edge: f32,
    /// A fold's chevron, sized to sit beside a row.
    pub fold_toggle: f32,
    /// How far a fold's rail sets its body in from the header.
    pub fold_indent: f32,
    /// A menu row's own corner, fixed rather than set per call site
    /// so every menu rounds the same.
    pub menu_item_radius: f32,
    /// A menu's own padding around its rows.
    pub menu_padding: f32,
    /// A menu's own corner - concentric with `menu_item_radius`
    /// across `menu_padding`; see `toolbars.md` in the Apple HIG.
    pub menu_radius: f32,
    /// How close a menu is allowed to sit to the window's edge
    /// before it flips to the other side.
    pub menu_margin: f32,
    /// A component card's own corner, in the entity inspector.
    pub card_radius: f32,
    /// A component card's padding around its header and fields.
    pub card_padding: f32,
}

/// `GlobalZIndex` levels, so a drag's chrome stacks the same way
/// wherever it is dragged.
#[derive(Clone, Copy, Debug)]
pub struct Layers {
    /// A drop target hint.
    pub drop_hint: i32,
    /// A dragged ghost.
    pub drag: i32,
    /// A right-click menu.
    pub context_menu: i32,
    /// A tooltip.
    pub tooltip: i32,
}

/// Font sizes, three steps.
#[derive(Clone, Copy, Debug)]
pub struct TextScale {
    pub small: f32,
    pub body: f32,
    pub label: f32,
}

/// How the UI moves.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// How long a hover or press fade takes.
    pub interact: Duration,
    /// The curve an interaction fade follows.
    pub ease: EaseFn,
}

impl Default for EditorTheme {
    fn default() -> Self {
        let palette = Palette::default();
        let base = palette.base;
        Self {
            color: Colors {
                text: base[8],
                text_dim: base[6],
                text_faint: base[8].with_alpha(0.6),
                accent: palette.blue,
                critical: palette.red,
                bg: base[0],
                panel: base[1],
                fill: base[8].with_alpha(0.06),
                fill_faint: base[8].with_alpha(0.03),
                hairline: base[8].with_alpha(0.08),
                hover: base[8].with_alpha(0.14),
                selection: palette.blue.with_alpha(0.18),
                clip: palette.blue.with_alpha(0.5),
                clip_hover: Color::srgb(0.35, 0.70, 1.0),
                clip_press: Color::srgb(0.55, 0.82, 1.0),
            },
            space: Spacing {
                xs: 2.0,
                sm: 4.0,
                md: 6.0,
                lg: 8.0,
                xl: 12.0,
                radius: 4.0,
                row: 24.0,
                action_row: 32.0,
                lane_gap: 2.0,
                touch: 26.0,
                icon: 11.0,
                hairline: 1.0,
                edge: 2.0,
                fold_toggle: 14.0,
                fold_indent: 9.0,
                menu_item_radius: 6.0,
                menu_padding: 4.0,
                // Concentric with `menu_item_radius` across
                // `menu_padding`: 6.0 + 4.0.
                menu_radius: 10.0,
                menu_margin: 8.0,
                card_radius: 6.0,
                card_padding: 6.0,
            },
            text: TextScale {
                small: 10.0,
                body: 12.0,
                label: 14.0,
            },
            motion: Motion {
                interact: Duration::from_millis(120),
                ease: ease::cubic::ease_out,
            },
            layer: Layers {
                drop_hint: 150,
                drag: 200,
                context_menu: 250,
                tooltip: 300,
            },
            palette,
        }
    }
}

impl TextTokens for EditorTheme {
    fn tone(&self, tone: Tone) -> Color {
        match tone {
            Tone::Body => self.color.text,
            Tone::Dim => self.color.text_dim,
            Tone::Faint => self.color.text_faint,
            Tone::Accent => self.color.accent,
            Tone::Critical => self.color.critical,
            // The accent is light, so text on it is the ground
            // colour.
            Tone::OnAccent => self.color.bg,
        }
    }

    fn body_size(&self) -> f32 {
        self.text.body
    }

    fn small_size(&self) -> f32 {
        self.text.small
    }
}

impl SurfaceTokens for EditorTheme {
    fn fill(&self) -> Color {
        self.color.fill
    }

    fn hover(&self) -> Color {
        self.color.hover
    }

    fn panel(&self) -> Color {
        self.color.panel
    }

    fn hairline(&self) -> Color {
        self.color.hairline
    }

    fn selection(&self) -> Color {
        self.color.selection
    }

    fn accent(&self) -> Color {
        self.color.accent
    }
}

impl SpacingTokens for EditorTheme {
    fn gap(&self) -> f32 {
        self.space.md
    }

    fn row(&self) -> f32 {
        self.space.row
    }

    fn radius(&self) -> f32 {
        self.space.radius
    }

    fn menu_radius(&self) -> f32 {
        self.space.menu_radius
    }

    fn menu_padding(&self) -> f32 {
        self.space.menu_padding
    }

    fn menu_item_radius(&self) -> f32 {
        self.space.menu_item_radius
    }

    fn menu_margin(&self) -> f32 {
        self.space.menu_margin
    }
}

/// Every kind of motion follows the one interaction curve, since the
/// theme has no other.
impl MotionTokens for EditorTheme {
    fn motion(&self, motion: MotionKind) -> Curve {
        match motion {
            MotionKind::Interact | MotionKind::Expand => Curve {
                duration: self.motion.interact,
                ease: self.motion.ease,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_map_to_their_colors() {
        let theme = EditorTheme::default();
        let color = theme.color;
        assert_eq!(theme.tone(Tone::Body), color.text);
        assert_eq!(theme.tone(Tone::Dim), color.text_dim);
        assert_eq!(theme.tone(Tone::Faint), color.text_faint);
        assert_eq!(theme.tone(Tone::Accent), color.accent);
        assert_eq!(theme.tone(Tone::Critical), color.critical);
        assert_eq!(theme.body_size(), theme.text.body);
        assert_eq!(theme.small_size(), theme.text.small);
    }

    #[test]
    fn surfaces_map_to_their_colors() {
        let theme = EditorTheme::default();
        let color = theme.color;
        assert_eq!(SurfaceTokens::fill(&theme), color.fill);
        assert_eq!(SurfaceTokens::hover(&theme), color.hover);
        assert_eq!(SurfaceTokens::panel(&theme), color.panel);
        assert_eq!(SurfaceTokens::hairline(&theme), color.hairline);
        assert_eq!(SurfaceTokens::selection(&theme), color.selection);
        assert_eq!(SurfaceTokens::accent(&theme), color.accent);
    }

    #[test]
    fn spacing_maps_to_its_scale() {
        let theme = EditorTheme::default();
        let space = theme.space;
        assert_eq!(theme.gap(), space.md);
        assert_eq!(theme.row(), space.row);
        assert_eq!(SpacingTokens::radius(&theme), space.radius);
        assert_eq!(theme.menu_radius(), space.menu_radius);
        assert_eq!(theme.menu_padding(), space.menu_padding);
        assert_eq!(theme.menu_item_radius(), space.menu_item_radius);
        assert_eq!(theme.menu_margin(), space.menu_margin);
    }

    #[test]
    fn every_motion_uses_the_interaction_curve() {
        let theme = EditorTheme::default();
        for motion in [MotionKind::Interact, MotionKind::Expand] {
            let curve = theme.motion(motion);
            assert_eq!(curve.duration, theme.motion.interact);
            assert_eq!((curve.ease)(0.5), (theme.motion.ease)(0.5));
        }
    }
}
