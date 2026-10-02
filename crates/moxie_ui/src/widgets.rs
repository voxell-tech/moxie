//! Widgets that `bevy_fynix` provides, re-exported for one import
//! path.

pub use bevy_fynix::dock;

/// The tooltip view and its timing.
pub mod tooltip {
    pub use bevy_fynix::views::{Tooltip, TooltipExt, TooltipTiming};
}
