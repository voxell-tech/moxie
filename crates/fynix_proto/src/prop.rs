//! A value a view is handed.

use alloc::boxed::Box;
use alloc::string::{String, ToString};

/// A prop: set at the call site, bound to the world `W`, or left for
/// set rules and then the theme to decide.
#[derive(Default)]
pub enum Prop<W, T> {
    #[default]
    Unset,
    Value(T),
    Bound(Signal<W, T>),
}

/// A value read from the world `W`, re-read while its view is mounted.
pub struct Signal<W, T>(Box<dyn Fn(&W) -> T + Send + Sync>);

/// A prop that follows whatever `read` returns.
pub fn derived<W, T>(
    read: impl Fn(&W) -> T + Send + Sync + 'static,
) -> Signal<W, T> {
    Signal(Box::new(read))
}

impl<W, T> Prop<W, T> {
    pub fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }

    pub fn is_bound(&self) -> bool {
        matches!(self, Self::Bound(_))
    }

    /// This, or `below` when this was left unset.
    pub fn or(self, below: Self) -> Self {
        match self {
            Self::Unset => below,
            set => set,
        }
    }

    /// What this holds now. `None` when unset.
    pub fn get(&self, world: &W) -> Option<T>
    where
        T: Clone,
    {
        match self {
            Self::Unset => None,
            Self::Value(value) => Some(value.clone()),
            Self::Bound(signal) => Some((signal.0)(world)),
        }
    }
}

impl<W, T> From<T> for Prop<W, T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

impl<W, T> From<Signal<W, T>> for Prop<W, T> {
    fn from(signal: Signal<W, T>) -> Self {
        Self::Bound(signal)
    }
}

impl<W> From<&str> for Prop<W, String> {
    fn from(text: &str) -> Self {
        Self::Value(text.to_string())
    }
}
