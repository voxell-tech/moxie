#![doc = include_str!("../README.md")]
#![allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "Inherent to Bevy ECS: systems take many params and \
              query tuples."
)]

pub mod asset;
pub mod asset_picker;
pub mod context_menu;
pub mod cursor;
pub mod drag;
pub mod elements;
pub mod field_icon;
pub mod fold;
pub mod gaps;
pub mod icons;
pub mod inspector;
pub mod layout;
#[cfg(test)]
mod tests;
pub mod theme;
pub mod widgets;

use asset::AssetDragging;
use bevy::prelude::*;
use bevy_fynix::dock::DockPlugin;
use bevy_fynix::{FynixPlugin, Theme};
use inspector::InspectPlugin;
use moxie_asset::{AssetTypes, FoundAssets};
use theme::EditorTheme;

/// Everything a consumer needs to render a moxie UI: the fynix
/// kernel and the dock, themed by [`EditorTheme`].
///
/// Doesn't mount a root itself; call [`bevy_fynix::mount`] wherever
/// the app does its own `Startup` setup.
#[derive(Default)]
pub struct MoxieUiPlugin;

impl Plugin for MoxieUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FynixPlugin::<EditorTheme>::default(),
            DockPlugin::<EditorTheme>::default(),
            InspectPlugin,
            asset_picker::plugin,
        ))
        .insert_resource(Theme(EditorTheme::default()))
        .add_systems(Update, elements::fit_action_icons)
        .init_resource::<AssetTypes>()
        .init_resource::<FoundAssets>()
        .init_resource::<AssetDragging>();
    }
}
