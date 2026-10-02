use std::any::TypeId;
use std::collections::HashMap;
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
/// [`TypeId`], in the order they were first registered. Filled in
/// through [`AssetTypeAppExt::asset_type`].
#[derive(Resource, Default)]
pub struct AssetTypes(Vec<(TypeId, AssetType)>);

/// What the editor knows about one asset type.
#[derive(Clone, Default)]
pub struct AssetType {
    /// File extensions that load as it.
    pub extensions: Vec<&'static str>,
    /// Built-in assets offered wherever one is picked.
    pub choices: Vec<AssetChoice>,
    /// Whether a field must always hold one, so its picker offers no
    /// "None".
    pub required: bool,
    pub thumbnail: Option<RenderThumbnail>,
    /// What the picker's "New" makes one with.
    pub create: Option<CreateAsset>,
}

impl AssetTypes {
    pub fn get(&self, kind: TypeId) -> Option<&AssetType> {
        self.0
            .iter()
            .find(|(known, _)| *known == kind)
            .map(|(_, info)| info)
    }

    /// The type `path`'s extension loads as, if any. When two claim
    /// it, the one registered last.
    pub fn kind_of(&self, path: &Path) -> Option<TypeId> {
        let extension = path.extension()?.to_str()?;
        self.0
            .iter()
            .rev()
            .find(|(_, info)| {
                info.extensions.iter().any(|known| {
                    known.eq_ignore_ascii_case(extension)
                })
            })
            .map(|(kind, _)| *kind)
    }
}

pub trait AssetTypeAppExt {
    /// What the editor knows about `T`, to fill in.
    fn asset_type<T: Asset>(&mut self) -> Mut<'_, AssetType>;
}

impl AssetTypeAppExt for App {
    fn asset_type<T: Asset>(&mut self) -> Mut<'_, AssetType> {
        self.world_mut()
            .get_resource_or_insert_with(AssetTypes::default)
            .map_unchanged(|types| {
                let kind = TypeId::of::<T>();
                let index = match types
                    .0
                    .iter()
                    .position(|(known, _)| *known == kind)
                {
                    Some(index) => index,
                    None => {
                        types.0.push((kind, AssetType::default()));
                        types.0.len() - 1
                    }
                };
                &mut types.0[index].1
            })
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

    /// A handle to the asset, loading it when it is a file. A file
    /// may lie outside the default asset source.
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

/// One pickable asset.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetChoice {
    pub name: String,
    pub asset: AssetRef,
    /// What the choice is listed under.
    pub group: String,
}

/// Assets found in and around the project, by the asset's own
/// [`TypeId`]: the choices that change while the editor runs.
#[derive(Resource, Default)]
pub struct FoundAssets(pub HashMap<TypeId, Vec<AssetChoice>>);

/// Every choice for a `T`: the found ones, then the built-in ones.
pub fn asset_choices<T: Asset>(
    world: &World,
) -> impl Iterator<Item = &AssetChoice> {
    let kind = TypeId::of::<T>();
    let found = world
        .get_resource::<FoundAssets>()
        .and_then(|found| found.0.get(&kind));
    let built_in = world
        .get_resource::<AssetTypes>()
        .and_then(|types| types.get(kind))
        .map(|info| &info.choices);
    found.into_iter().chain(built_in).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_choices_come_before_built_in_ones() {
        let mut app = App::new();
        let choice = |name: &str| AssetChoice {
            name: name.to_string(),
            asset: AssetRef::Uuid(Uuid::nil()),
            group: String::new(),
        };
        app.asset_type::<Image>().choices.push(choice("Blank"));
        app.world_mut().insert_resource(FoundAssets(HashMap::from(
            [(TypeId::of::<Image>(), vec![choice("Found")])],
        )));

        let names = asset_choices::<Image>(app.world())
            .map(|choice| choice.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Found", "Blank"]);
    }

    #[test]
    fn an_extension_matches_in_any_case() {
        let mut app = App::new();
        app.asset_type::<Image>().extensions.push("png");

        let types = app.world().resource::<AssetTypes>();
        let kind = Some(TypeId::of::<Image>());
        assert_eq!(types.kind_of(Path::new("a/b.PNG")), kind);
        assert_eq!(types.kind_of(Path::new("a/b.jpg")), None);
    }

    #[test]
    fn a_shared_extension_goes_to_the_last_registered() {
        let mut app = App::new();
        app.asset_type::<Image>().extensions.push("dat");
        app.asset_type::<Mesh>().extensions.push("dat");
        // Filling in the first again doesn't move it.
        app.asset_type::<Image>().required = true;

        let types = app.world().resource::<AssetTypes>();
        assert_eq!(
            types.kind_of(Path::new("a.dat")),
            Some(TypeId::of::<Mesh>())
        );
    }
}
