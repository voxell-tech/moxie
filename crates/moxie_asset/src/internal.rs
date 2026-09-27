use std::any::TypeId;

use bevy::asset::uuid::Uuid;
use bevy::asset::{Asset, ReflectAsset, UntypedAssetId};
use bevy::prelude::*;

use crate::project::{LoadedAsset, SavedAsset};

/// Assets the project owns rather than references: each kept in its
/// `Assets` under a UUID, never loaded from a file, and saved inside
/// the project file.
#[derive(Resource, Default)]
pub struct InternalAssets {
    entries: Vec<InternalAsset>,
}

/// One internal asset. Its value lives in `Assets`, under `id`.
#[derive(Clone, Debug, PartialEq)]
pub struct InternalAsset {
    pub id: Uuid,
    pub kind: TypeId,
    pub name: String,
}

impl InternalAssets {
    /// Keeps `value` as a new internal asset named `name`.
    pub fn add<T: Asset>(
        &mut self,
        assets: &mut Assets<T>,
        name: String,
        value: T,
    ) -> Handle<T> {
        let id = Uuid::new_v4();
        // A UUID id can't be stale, which is the only way this fails.
        let _ = assets.insert(id, value);
        self.entries.push(InternalAsset {
            id,
            kind: TypeId::of::<T>(),
            name,
        });
        Handle::from(id)
    }

    /// Records an asset already put in its `Assets` under `id`.
    pub fn adopt(&mut self, asset: InternalAsset) {
        self.entries.retain(|entry| entry.id != asset.id);
        self.entries.push(asset);
    }

    pub fn get(&self, id: Uuid) -> Option<&InternalAsset> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &InternalAsset> {
        self.entries.iter()
    }

    /// `base`, numbered past every `kind` asset already named after it.
    pub fn unused_name(&self, kind: TypeId, base: &str) -> String {
        let taken = |name: &str| {
            self.entries
                .iter()
                .any(|entry| entry.kind == kind && entry.name == name)
        };
        if !taken(base) {
            return base.to_string();
        }
        (1..)
            .map(|n| format!("{base} {n}"))
            .find(|name| !taken(name))
            .unwrap_or_default()
    }

    /// Every internal asset with its value, borrowed out of `world`,
    /// to be saved. One whose value has gone is left out.
    pub fn to_save<'w>(
        &'w self,
        world: &'w World,
    ) -> Vec<SavedAsset<'w>> {
        self.entries
            .iter()
            .filter_map(|asset| {
                let value = reflect_asset(world, asset.kind)?
                    .get(world, asset.untyped())?;
                Some(SavedAsset {
                    id: asset.id,
                    name: &asset.name,
                    value: value.as_partial_reflect(),
                })
            })
            .collect()
    }
}

impl InternalAsset {
    fn untyped(&self) -> UntypedAssetId {
        UntypedAssetId::Uuid {
            type_id: self.kind,
            uuid: self.id,
        }
    }
}

/// Drops every internal asset in `world`, values and all, and keeps
/// `loaded` in their place. One whose type is unknown is left out.
pub fn replace_internal_assets(
    world: &mut World,
    loaded: Vec<LoadedAsset>,
) {
    let old = std::mem::take(
        &mut world.resource_mut::<InternalAssets>().entries,
    );
    for asset in old {
        if let Some(reflect) = reflect_asset(world, asset.kind) {
            reflect.remove(world, asset.untyped());
        }
    }

    for asset in loaded {
        let Some(kind) = asset
            .value
            .get_represented_type_info()
            .map(|info| info.type_id())
        else {
            warn!("internal asset {} has no known type", asset.name);
            continue;
        };
        let Some(reflect) = reflect_asset(world, kind) else {
            warn!(
                "internal asset {} is not a registered asset",
                asset.name
            );
            continue;
        };
        let entry = InternalAsset {
            id: asset.id,
            kind,
            name: asset.name,
        };
        if let Err(err) =
            reflect.insert(world, entry.untyped(), &*asset.value)
        {
            warn!(
                "internal asset {} could not be kept: {err}",
                entry.name
            );
            continue;
        }
        world.resource_mut::<InternalAssets>().adopt(entry);
    }
}

/// `kind`'s [`ReflectAsset`], cloned out so the registry guard is never
/// held while the caller runs.
fn reflect_asset(
    world: &World,
    kind: TypeId,
) -> Option<ReflectAsset> {
    let registry = world.resource::<AppTypeRegistry>().read();
    registry.get_type_data::<ReflectAsset>(kind).cloned()
}
