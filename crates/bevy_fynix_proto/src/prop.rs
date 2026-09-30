//! A value a view is handed.

use bevy::prelude::*;

/// A prop: set at the call site, bound to the world, or left for set
/// rules and then the theme to decide.
#[derive(Default)]
pub enum Prop<T> {
    #[default]
    Unset,
    Value(T),
    Bound(Signal<T>),
}

/// A value read from the world, re-read every frame.
pub struct Signal<T>(Box<dyn Fn(&World) -> T + Send + Sync>);

/// A prop that follows whatever `read` returns.
pub fn derived<T>(
    read: impl Fn(&World) -> T + Send + Sync + 'static,
) -> Signal<T> {
    Signal(Box::new(read))
}

impl<T> Prop<T> {
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
    pub fn get(&self, world: &World) -> Option<T>
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

impl<T> From<T> for Prop<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

impl<T> From<Signal<T>> for Prop<T> {
    fn from(signal: Signal<T>) -> Self {
        Self::Bound(signal)
    }
}

impl From<&str> for Prop<String> {
    fn from(text: &str) -> Self {
        Self::Value(text.to_string())
    }
}
