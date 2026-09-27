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
/// [`TypeId`]: the ones registered up front, and the ones found in
/// the project.
#[derive(Resource, Default)]
pub struct AssetChoices {
    registered: HashMap<TypeId, Vec<AssetChoice>>,
    found: HashMap<TypeId, Vec<AssetChoice>>,
}

/// One pickable asset.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetChoice {
    pub name: String,
    /// An [`AssetPath`](bevy::asset::AssetPath), as a string.
    pub path: String,
    /// What the choice is listed under.
    pub group: String,
}

impl AssetChoices {
    /// Every choice for `T`, found ones first.
    pub fn of<T: Asset>(&self) -> impl Iterator<Item = &AssetChoice> {
        let kind = TypeId::of::<T>();
        [&self.found, &self.registered]
            .into_iter()
            .filter_map(move |choices| choices.get(&kind))
            .flatten()
    }

    /// Whether the found choices are already `found`.
    pub fn found_is(
        &self,
        found: &HashMap<TypeId, Vec<AssetChoice>>,
    ) -> bool {
        self.found == *found
    }

    /// Replaces every found choice.
    pub fn set_found(
        &mut self,
        found: HashMap<TypeId, Vec<AssetChoice>>,
    ) {
        self.found = found;
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
    /// picked, listed under `group`.
    fn register_asset_choices<T: Asset>(
        &mut self,
        group: &str,
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
        group: &str,
        choices: &[(&str, &str)],
    ) -> &mut Self {
        let mut all = self
            .world_mut()
            .get_resource_or_insert_with(AssetChoices::default);
        all.registered.entry(TypeId::of::<T>()).or_default().extend(
            choices.iter().map(|(name, path)| AssetChoice {
                name: name.to_string(),
                path: path.to_string(),
                group: group.to_string(),
            }),
        );
        self
    }
}
