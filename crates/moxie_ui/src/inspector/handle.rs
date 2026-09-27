//! [`Inspect`] for a [`Handle<T>`], for any asset `T`.
//!
//! One row, showing whatever asset is currently assigned, and a drop
//! target for a file dragged from the assets panel whose registered
//! [`moxie_asset::AssetKinds`] kind matches `T`. A `T` with registered
//! [`AssetChoices`] also opens a list of them to pick from.

use std::any::TypeId;

use bevy::asset::{Asset, AssetPath};
use bevy::picking::events::{DragDrop, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use bevy_fynix::WorldEntityMut;
use bevy_fynix::tag::TagExt as _;
use fynix::prelude::*;
use moxie_asset::{ABSOLUTE_SOURCE, AssetChoice, AssetChoices};

use crate::asset::AssetDragging;
use crate::elements::{
    ButtonCursor, Dropdown, DropdownCursor, DropdownList,
    DropdownMenu, GhostButton, Icon, Label, LabelCursor, menu_item,
};
use crate::icons;
use crate::reactive::BevyUi;

use super::{
    ClonableSource, Inspect, Source, SourceExt, when_changed,
};

impl<T: Asset + TypePath> Inspect for Handle<T> {
    fn build(source: &dyn Source, ui: &mut BevyUi) {
        let choices = ui
            .world
            .get_resource::<AssetChoices>()
            .map(|choices| choices.of::<T>().to_vec())
            .unwrap_or_default();

        if choices.is_empty() {
            slot::<T>(source, ui);
        } else {
            picker::<T>(source, ui, choices);
        }
    }
}

/// The assigned asset, filled only by a drop.
fn slot<T: Asset>(source: &dyn Source, ui: &mut BevyUi) {
    let read = source.boxed();
    let muted = ui.theme.color.text_dim;
    let label = label_of::<T>(ui.world, source, &[]);

    let mut slot = ui.elem(elem!(
        !GhostButton,
        width = percent(100),
        justify = JustifyContent::SpaceBetween,
        icon = elem!(Icon, image = icons::ASSET, color = muted),
        label =
            elem!(Label, text = label, color = muted, wrap = false)
    ));
    slot.bind(
        |button| button.label().text(),
        when_changed(source),
        move |WorldNodeRef { world, .. }| {
            label_of::<T>(world, &*read, &[])
        },
    );
    accept_drop::<T>(&mut slot, source);
}

/// The assigned asset, as a dropdown over `choices`. Still takes a
/// drop, for an asset none of them name.
fn picker<T: Asset>(
    source: &dyn Source,
    ui: &mut BevyUi,
    choices: Vec<AssetChoice>,
) {
    let read = source.boxed();
    let written = source.boxed();
    let text = ui.theme.color.text;
    let text_dim = ui.theme.color.text_dim;
    let label = label_of::<T>(ui.world, source, &choices);
    let width = Dropdown::width_for(
        &choices
            .iter()
            .map(|choice| choice.name.clone())
            .collect::<Vec<_>>(),
        12.0,
    );

    ui.elem(elem!(DropdownMenu)).with(move |ui| {
        let shown = choices.clone();
        let mut control = ui.elem(elem!(
            Dropdown,
            min_width = width,
            label = elem!(
                Label,
                text = label,
                wrap = false,
                color = text
            ),
            chevron = elem!(
                Icon,
                image = icons::CHEVRON,
                color = text_dim,
                size = px(9),
                rotation = 180.0f32
            )
        ));
        control.pointer_tags().bind(
            |dropdown| dropdown.label().text(),
            when_changed(&*read),
            move |WorldNodeRef { world, .. }| {
                label_of::<T>(world, &*read, &shown)
            },
        );
        accept_drop::<T>(&mut control, &*written);

        let source = ClonableSource(written.boxed());
        ui.elem(elem!(DropdownList, width = width)).with(move |ui| {
            for choice in &choices {
                let source = source.clone();
                let path = choice.path.clone();
                menu_item(
                    ui,
                    None,
                    choice.name.clone(),
                    move |world| {
                        let handle = world
                            .resource::<AssetServer>()
                            .load::<T>(path.clone());
                        source.set(world, &handle);
                    },
                );
            }
        });
    });
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

/// What `source` currently holds - the name of the choice it matches,
/// else the asset's own path if the server knows one, or a
/// placeholder for a handle with none or nothing assigned at all.
fn label_of<T: Asset>(
    world: &World,
    source: &dyn Source,
    choices: &[AssetChoice],
) -> String {
    let Some(handle) = source.read::<Handle<T>>(world) else {
        return "(none)".to_string();
    };
    let Some(path) = world
        .get_resource::<AssetServer>()
        .and_then(|assets| assets.get_path(&handle))
    else {
        return "(unnamed)".to_string();
    };

    let path = path.to_string();
    choices
        .iter()
        .find(|choice| choice.path == path)
        .map(|choice| choice.name.clone())
        .unwrap_or(path)
}
