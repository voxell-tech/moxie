//! Materials the project owns: made from the asset picker's "New",
//! starting from whichever material the field held.

use std::any::TypeId;

use bevy::asset::{UntypedAssetId, UntypedHandle};
use bevy::prelude::*;
use moxie_asset::{AssetTypeAppExt as _, InternalAssets};

pub(crate) fn plugin(app: &mut App) {
    app.asset_type::<StandardMaterial>().create = Some(new_material);
}

/// A new internal material, a copy of `seed`'s when there is one.
fn new_material(
    world: &mut World,
    seed: Option<UntypedAssetId>,
) -> Option<UntypedHandle> {
    let value = seed
        .and_then(|id| {
            world
                .resource::<Assets<StandardMaterial>>()
                .get(id.typed::<StandardMaterial>())
                .cloned()
        })
        .unwrap_or_default();

    world.resource_scope(
        |world, mut internal: Mut<InternalAssets>| {
            let name = internal.unused_name(
                TypeId::of::<StandardMaterial>(),
                "Material",
            );
            let mut materials =
                world.resource_mut::<Assets<StandardMaterial>>();
            Some(internal.add(&mut materials, name, value).untyped())
        },
    )
}
