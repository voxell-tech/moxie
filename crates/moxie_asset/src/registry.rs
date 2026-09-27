use std::any::TypeId;
use std::collections::HashMap;
use std::path::Path;

use bevy::asset::Asset;
use bevy::prelude::*;

/// Which file extension loads as which [`Asset`], by that asset's
/// own [`TypeId`].
#[derive(Resource, Default)]
pub struct AssetKinds {
    by_extension: HashMap<String, TypeId>,
}

impl AssetKinds {
    /// The registered kind `path`'s extension loads as, if any.
    pub fn kind_of(&self, path: &Path) -> Option<TypeId> {
        let extension = path.extension()?.to_str()?.to_lowercase();
        self.by_extension.get(&extension).copied()
    }
}

/// Named asset paths offered for picking, by the asset's own
/// [`TypeId`].
#[derive(Resource, Default)]
pub struct AssetChoices {
    by_kind: HashMap<TypeId, Vec<AssetChoice>>,
}

/// One pickable asset.
#[derive(Clone, Debug)]
pub struct AssetChoice {
    pub name: String,
    /// Relative to the default asset source.
    pub path: String,
}

impl AssetChoices {
    /// Every choice registered for `T`, in registration order.
    pub fn of<T: Asset>(&self) -> &[AssetChoice] {
        self.by_kind
            .get(&TypeId::of::<T>())
            .map_or(&[], Vec::as_slice)
    }
}

/// Registering what a file extension loads as.
pub trait AssetKindAppExt {
    /// Marks every extension in `extensions` as loading a `T`.
    fn register_asset_kind<T: Asset>(
        &mut self,
        extensions: &[&str],
    ) -> &mut Self;

    /// Offers every `(name, path)` in `choices` wherever a `T` is
    /// picked.
    fn register_asset_choices<T: Asset>(
        &mut self,
        choices: &[(&str, &str)],
    ) -> &mut Self;
}

impl AssetKindAppExt for App {
    fn register_asset_kind<T: Asset>(
        &mut self,
        extensions: &[&str],
    ) -> &mut Self {
        let mut kinds = self
            .world_mut()
            .get_resource_or_insert_with(AssetKinds::default);
        for extension in extensions {
            kinds
                .by_extension
                .insert(extension.to_lowercase(), TypeId::of::<T>());
        }
        self
    }

    fn register_asset_choices<T: Asset>(
        &mut self,
        choices: &[(&str, &str)],
    ) -> &mut Self {
        let mut registered = self
            .world_mut()
            .get_resource_or_insert_with(AssetChoices::default);
        registered
            .by_kind
            .entry(TypeId::of::<T>())
            .or_default()
            .extend(choices.iter().map(|(name, path)| AssetChoice {
                name: name.to_string(),
                path: path.to_string(),
            }));
        self
    }
}
