//! Values travelling to a new target over a curve.

use core::time::Duration;

use bevy::prelude::*;

use crate::tokens::Curve;

/// A value that can be blended between two of itself.
pub trait Interpolate: Sized {
    /// The blend of `from` and `to` at `t`, from 0 to 1. A part that
    /// cannot blend takes `to`.
    fn lerp(from: &Self, to: &Self, t: f32) -> Self;
}

impl Interpolate for f32 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }
}

/// Blended in Oklab, whatever space either colour is stored in.
impl Interpolate for Color {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Oklaba::from(*from).mix(&Oklaba::from(*to), t).into()
    }
}

/// When set, every transition finishes at once.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct ReducedMotion(pub bool);

/// How one leaf's snapshots travel: over `curve`, blended by `lerp`.
pub struct Tween<S> {
    pub curve: Curve,
    pub lerp: fn(&S, &S, f32) -> S,
}

/// A transition under way from `from` to whatever the target is.
pub(crate) struct Run<S> {
    from: S,
    tween: Tween<S>,
    elapsed: Duration,
}

impl<S> Run<S> {
    pub(crate) fn new(from: S, tween: Tween<S>) -> Self {
        Self {
            from,
            tween,
            elapsed: Duration::ZERO,
        }
    }

    /// Moves `delta` on, and returns the value now, or `None` once it
    /// has reached `to`.
    pub(crate) fn advance(
        &mut self,
        delta: Duration,
        to: &S,
        reduced: bool,
    ) -> Option<S> {
        self.elapsed += delta;
        let total = self.tween.curve.duration;
        if reduced || self.elapsed >= total {
            return None;
        }
        let progress =
            self.elapsed.as_secs_f32() / total.as_secs_f32();
        let t = (self.tween.curve.ease)(progress);
        Some((self.tween.lerp)(&self.from, to, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_and_colors_blend() {
        assert_eq!(
            <f32 as Interpolate>::lerp(&10.0, &20.0, 0.25),
            12.5
        );
        let black = Color::BLACK;
        let white = Color::srgb(1.0, 1.0, 1.0);

        let end = Color::lerp(&black, &white, 1.0).to_srgba();
        assert!((end.red - 1.0).abs() < 1e-4);
        let mid = Color::lerp(&black, &white, 0.5).to_srgba();
        assert!(mid.red > 0.0 && mid.red < 1.0);
        assert!((mid.red - mid.green).abs() < 1e-4);
    }
}
