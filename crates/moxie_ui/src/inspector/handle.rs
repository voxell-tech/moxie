//! [`Inspect`] for a [`Handle<T>`], for any asset `T`.
//!
//! One row, showing whatever asset is currently assigned. Clicking it
//! opens the [asset picker](crate::asset_picker), and a file dragged
//! from the assets panel whose registered [`moxie_asset::AssetKinds`]
//! kind matches `T` can be dropped on it.

use std::any::TypeId;

use bevy::asset::{Asset, AssetPath};
use bevy::picking::events::{DragDrop, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use bevy_fynix::WorldEntityMut;
use fynix::prelude::*;
use moxie_asset::{ABSOLUTE_SOURCE, AssetChoices};

use crate::asset::AssetDragging;
use crate::asset_picker::open_asset_picker;
use crate::cursor::Cursor;
use crate::elements::{
    ButtonCursor, GhostButton, Icon, Label, LabelCursor,
};
use crate::icons;
use crate::reactive::BevyUi;

use super::{
    ClonableSource, Inspect, Source, SourceExt, when_changed,
};

impl<T: Asset + TypePath> Inspect for Handle<T> {
    fn build(source: &dyn Source, ui: &mut BevyUi) {
        let read = source.boxed();
        let picked = ClonableSource(source.boxed());
        let muted = ui.theme.color.text_dim;
        let label = label_of::<T>(ui.world, source);

        let mut slot = ui.elem(elem!(
            !GhostButton,
            width = percent(100),
            justify = JustifyContent::SpaceBetween,
            icon = elem!(Icon, image = icons::ASSET, color = muted),
            label = elem!(
                Label,
                text = label,
                color = muted,
                wrap = false
            )
        ));
        slot.bind(
            |button| button.label().text(),
            when_changed(source),
            move |WorldNodeRef { world, .. }| {
                label_of::<T>(world, &*read)
            },
        )
        .observe(
            move |_: On<Activate>,
                  cursor: Cursor,
                  mut commands: Commands| {
                let Some(at) = cursor.position() else {
                    return;
                };
                let source = picked.clone();
                commands.queue(move |world: &mut World| {
                    open_asset_picker::<T>(world, at, source);
                });
            },
        );
        accept_drop::<T>(&mut slot, source);
    }
}

/// Loads a file dragged from the assets panel into `source`, when its
/// registered kind is `T`.
fn accept_drop<T: Asset>(
    elem: &mut impl WorldEntityMut,
    source: &dyn Source,
) {
    let written = source.boxed();
    let kind = TypeId::of::<T>();

    elem.observe(
        move |drop: On<Pointer<DragDrop>>,
              dragging: Res<AssetDragging>,
              mut commands: Commands| {
            if drop.button != PointerButton::Primary {
                return;
            }
            let (Some(path), Some(dragged)) =
                (dragging.path.clone(), dragging.kind)
            else {
                return;
            };
            if dragged != kind {
                return;
            }

            let source = written.boxed();
            commands.queue(move |world: &mut World| {
                // Rooted at `/`: a dragged path is absolute and may
                // live anywhere on disk.
                let asset_path =
                    AssetPath::from_path_buf(path.clone())
                        .with_source(ABSOLUTE_SOURCE);
                // A dragged file's path is outside the configured
                // asset root by construction, so it needs `Deny`'s
                // per-load override; see `unapproved_path_mode` in
                // `main.rs`.
                let handle = world
                    .resource::<AssetServer>()
                    .load_builder()
                    .override_unapproved()
                    .load::<T>(asset_path);
                source.write(world, handle);
            });
        },
    );
}

/// What `source` currently holds - the name of the [`AssetChoices`]
/// entry it matches, else the asset's own path if the server knows
/// one, or a placeholder for a handle with none or nothing assigned
/// at all.
fn label_of<T: Asset>(world: &World, source: &dyn Source) -> String {
    let Some(handle) = source.read::<Handle<T>>(world) else {
        return "(none)".to_string();
    };
    let Some(path) = world
        .get_resource::<AssetServer>()
        .and_then(|assets| assets.get_path(&handle))
    else {
        return "(none)".to_string();
    };

    let path = path.to_string();
    world
        .get_resource::<AssetChoices>()
        .and_then(|choices| {
            choices
                .of::<T>()
                .iter()
                .find(|choice| choice.path == path)
        })
        .map(|choice| choice.name.clone())
        .unwrap_or(path)
}
