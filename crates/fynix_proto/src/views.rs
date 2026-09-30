//! The views the prototype ships.

mod button;
mod frame;
mod icon;
mod label;
mod stack;

pub use button::{Button, button};
pub use frame::{Frame, FrameSnapshot, frame};
pub use icon::{Icon, IconSnapshot, icon};
pub use label::{Label, LabelSnapshot, label};
pub use stack::{Stack, column, row};
