use std::any::TypeId;

use bevy::prelude::*;
use bevy::reflect::TypeData;

/// `type_id`'s `D`, cloned out so the registry's read guard is
/// dropped before the caller runs. Nesting two read guards on one
/// thread can deadlock the moment a writer queues between them.
pub fn type_data<D: TypeData + Clone>(
    world: &World,
    type_id: TypeId,
) -> Option<D> {
    let registry = world.resource::<AppTypeRegistry>().read();
    registry.get_type_data::<D>(type_id).cloned()
}
