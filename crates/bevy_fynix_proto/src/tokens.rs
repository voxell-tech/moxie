//! What views read from a theme, as small traits a theme implements.
//! A view bounds only the traits its defaults read, so it works under
//! any theme that can answer for them.

use core::time::Duration;

use bevy::prelude::*;

/// A text colour by role, so a view can ask for "dim" without knowing
/// what dim is in a given theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Body,
    Dim,
    Accent,
}

pub trait TextTokens {
    fn tone(&self, tone: Tone) -> Color;
    fn body_size(&self) -> f32;
    fn small_size(&self) -> f32;
}

pub trait SurfaceTokens {
    fn fill(&self) -> Color;
    fn hover(&self) -> Color;
    fn panel(&self) -> Color;
}

pub trait SpacingTokens {
    fn gap(&self) -> f32;
    fn row(&self) -> f32;
    fn radius(&self) -> f32;
}

/// A kind of movement, resolved to a [`Curve`] by the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// Answering the pointer: hover, press.
    Interact,
    /// Something opening or growing.
    Expand,
}

/// How a value travels to a new one.
#[derive(Clone, Copy, Debug)]
pub struct Curve {
    pub duration: Duration,
    /// Progress in, eased progress out, both from 0 to 1.
    pub ease: fn(f32) -> f32,
}

pub trait MotionTokens {
    fn motion(&self, motion: Motion) -> Curve;
}
