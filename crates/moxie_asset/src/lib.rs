#![doc = include_str!("../README.md")]

mod internal;
pub mod project;
mod reflect;
mod registry;

use bevy::asset::io::AssetSourceBuilder;
use bevy::prelude::*;

pub use internal::{
    InternalAsset, InternalAssets, replace_internal_assets,
};
pub use reflect::type_data;
pub use registry::{
    AssetChoice, AssetChoices, AssetRef, AssetType, AssetTypeAppExt,
    AssetTypeBuilder, AssetTypes, CreateAsset, RenderThumbnail,
};

pub struct MoxieAssetPlugin;

impl Plugin for MoxieAssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetTypes>()
            .init_resource::<AssetChoices>()
            .init_resource::<InternalAssets>();
    }
}

/// The [`AssetSourceId`](bevy::asset::io::AssetSourceId) a dragged
/// file loads through, rooted at `/` - a bookmark can point anywhere
/// on disk, past wherever `AssetPlugin::file_path` put the editor's
/// own configured root.
pub const ABSOLUTE_SOURCE: &str = "abs";

/// Registers [`ABSOLUTE_SOURCE`]. Must run before `DefaultPlugins`:
/// asset sources build when `AssetPlugin` does.
pub fn register_absolute_source(app: &mut App) {
    app.register_asset_source(
        ABSOLUTE_SOURCE,
        AssetSourceBuilder::platform_default("/", None),
    );
}
