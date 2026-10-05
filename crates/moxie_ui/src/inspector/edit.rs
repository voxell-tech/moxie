use bevy::prelude::*;
use bevy::reflect::PartialReflect;

use super::{Field, SourceExt};

/// One [`Field`] being edited over several writes, as a drag does:
/// begun, written any number of times, then committed or cancelled.
pub struct Edit<T> {
    field: Field,
    before: T,
}

impl<T: FromReflect + PartialReflect + Clone> Edit<T> {
    /// Starts an edit of `field`. `None` when it cannot be read as
    /// `T`.
    pub fn begin(world: &World, field: Field) -> Option<Self> {
        let before = SourceExt::read::<T>(&field, world)?;
        Some(Self { field, before })
    }

    pub fn field(&self) -> &Field {
        &self.field
    }

    /// The value the field held when the edit began.
    pub fn before(&self) -> &T {
        &self.before
    }

    /// Writes `value`, as an inspector editor does.
    pub fn write(&self, world: &mut World, value: T) {
        SourceExt::write(&self.field, world, value);
    }

    /// Ends the edit, keeping the last value written.
    pub fn commit(self) {
        // One undo step, from `before` to what the field holds now.
    }

    /// Ends the edit, putting back the value it began with.
    pub fn cancel(self, world: &mut World) {
        SourceExt::write(&self.field, world, self.before);
    }
}
