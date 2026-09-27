use std::any::TypeId;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::Path;

use bevy::asset::uuid::Uuid;
use bevy::asset::{
    Asset, AssetServer, UntypedAssetId, UntypedHandle,
};
use bevy::prelude::*;

/// Renders a preview of an asset into an image, or `None` when it
/// can't.
pub type RenderThumbnail =
    fn(&mut World, &AssetRef) -> Option<Handle<Image>>;

/// Makes a new asset, seeded from `seed` when there is one, and hands
/// back a handle to it.
pub type CreateAsset = fn(
    &mut World,
    seed: Option<UntypedAssetId>,
) -> Option<UntypedHandle>;

/// What the editor knows about each asset type, by the asset's own
/// [`TypeId`]. Set up once, through [`AssetTypeAppExt::asset_type`].
#[derive(Resource, Default)]
pub struct AssetTypes {
    types: HashMap<TypeId, AssetType>,
}

/// What the editor knows about one asset type.
#[derive(Clone, Default)]
pub struct AssetType {
    /// File extensions that load as it, lowercase.
    pub extensions: Vec<String>,
    /// Whether a field must always hold one, so its picker offers no
    /// "None".
    pub required: bool,
    pub thumbnail: Option<RenderThumbnail>,
    /// What the picker's "New" makes one with.
    pub create: Option<CreateAsset>,
}

impl AssetTypes {
    pub fn get(&self, kind: TypeId) -> Option<&AssetType> {
        self.types.get(&kind)
    }

    /// The type `path`'s extension loads as, if any.
    pub fn kind_of(&self, path: &Path) -> Option<TypeId> {
        let extension = path.extension()?.to_str()?.to_lowercase();
        self.types
            .iter()
            .find(|(_, info)| info.extensions.contains(&extension))
            .map(|(&kind, _)| kind)
    }
}

/// How an asset is reached.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssetRef {
    /// A file, as an [`AssetPath`](bevy::asset::AssetPath) string.
    Path(String),
    /// An asset kept in its `Assets` under a fixed id, never loaded.
    Uuid(Uuid),
}

impl AssetRef {
    /// What `handle` points at, if it can be named at all.
    pub fn of<T: Asset>(
        handle: &Handle<T>,
        assets: &AssetServer,
    ) -> Option<Self> {
        match handle {
            Handle::Uuid(uuid, _) => Some(Self::Uuid(*uuid)),
            Handle::Strong(_) => assets
                .get_path(handle)
                .map(|path| Self::Path(path.to_string())),
        }
    }

    /// A handle to the asset, loading it when it is a file. A file may
    /// lie outside the default asset source.
    pub fn handle<T: Asset>(
        &self,
        assets: &AssetServer,
    ) -> Handle<T> {
        match self {
            Self::Path(path) => assets
                .load_builder()
                .override_unapproved()
                .load(path.clone()),
            Self::Uuid(uuid) => Handle::from(*uuid),
        }
    }
}

/// Named assets offered for picking, by the asset's own [`TypeId`]:
/// the ones registered up front, and the ones found in the project.
#[derive(Resource, Default)]
pub struct AssetChoices {
    registered: HashMap<TypeId, Vec<AssetChoice>>,
    found: HashMap<TypeId, Vec<AssetChoice>>,
}

/// One pickable asset.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetChoice {
    pub name: String,
    pub asset: AssetRef,
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

pub trait AssetTypeAppExt {
    /// Sets up what the editor knows about `T`.
    fn asset_type<T: Asset>(&mut self) -> AssetTypeBuilder<'_, T>;
}

impl AssetTypeAppExt for App {
    fn asset_type<T: Asset>(&mut self) -> AssetTypeBuilder<'_, T> {
        AssetTypeBuilder {
            app: self,
            asset: PhantomData,
        }
    }
}

/// What [`AssetTypeAppExt::asset_type`] hands back: each call adds to
/// what is known about `T`.
pub struct AssetTypeBuilder<'a, T> {
    app: &'a mut App,
    asset: PhantomData<T>,
}

impl<T: Asset> AssetTypeBuilder<'_, T> {
    /// Files with any of `extensions` load as a `T`.
    pub fn extensions(self, extensions: &[&str]) -> Self {
        self.edit(|info| {
            info.extensions.extend(
                extensions
                    .iter()
                    .map(|extension| extension.to_lowercase()),
            );
        })
    }

    /// Leaves "None" out of `T`'s picker.
    pub fn required(self) -> Self {
        self.edit(|info| info.required = true)
    }

    pub fn thumbnail(self, render: RenderThumbnail) -> Self {
        self.edit(|info| info.thumbnail = Some(render))
    }

    /// Gives `T`'s picker a "New" that makes one with `create`.
    pub fn creator(self, create: CreateAsset) -> Self {
        self.edit(|info| info.create = Some(create))
    }

    /// Offers every `(name, asset)` in `choices` wherever a `T` is
    /// picked, listed under `group`.
    pub fn choices(
        self,
        group: &str,
        choices: impl IntoIterator<Item = (String, AssetRef)>,
    ) -> Self {
        let mut all = self
            .app
            .world_mut()
            .get_resource_or_insert_with(AssetChoices::default);
        all.registered.entry(TypeId::of::<T>()).or_default().extend(
            choices.into_iter().map(|(name, asset)| AssetChoice {
                name,
                asset,
                group: group.to_string(),
            }),
        );
        self
    }

    fn edit(self, edit: impl FnOnce(&mut AssetType)) -> Self {
        let mut types = self
            .app
            .world_mut()
            .get_resource_or_insert_with(AssetTypes::default);
        edit(types.types.entry(TypeId::of::<T>()).or_default());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_is_built_up_across_calls() {
        let mut app = App::new();
        app.asset_type::<Image>().extensions(&["PNG"]);
        app.asset_type::<Image>().required().choices(
            "Built-in",
            [("Blank".to_string(), AssetRef::Uuid(Uuid::nil()))],
        );

        let types = app.world().resource::<AssetTypes>();
        let kind = TypeId::of::<Image>();
        assert_eq!(types.kind_of(Path::new("a/b.png")), Some(kind));
        assert_eq!(types.kind_of(Path::new("a/b.jpg")), None);
        assert!(types.get(kind).is_some_and(|info| info.required));

        let choices = app.world().resource::<AssetChoices>();
        let names = choices
            .of::<Image>()
            .map(|choice| choice.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Blank"]);
    }
}
