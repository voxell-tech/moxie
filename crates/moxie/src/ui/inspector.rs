//! Inspects whatever is selected in the hierarchy: every reflectable
//! component of one entity, each under a collapsible header.

use bevy::asset::uuid::Uuid;
use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps as _, column, frame, label, scroll,
};
use bevy_fynix::{AnyView, Bevy, ViewExt as _, keyed, resource};
use moxie_asset::InternalAssets;
use moxie_ui::elements::{asset_card, entity_inspector};
use moxie_ui::gaps::{anchored, changing_under};
use moxie_ui::inspector::{Binding, Source, reflect_changed};
use moxie_ui::theme::EditorTheme;

use crate::SelectedEntity;

/// The inspector panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        cx.build(
            keyed::<EditorTheme, Option<Entity>>(
                resource::<SelectedEntity, _>(|selected| selected.0),
                |selected| match *selected {
                    Some(entity) => selection(entity),
                    None => label("Nothing selected")
                        .tone(Tone::Dim)
                        .boxed(),
                },
            )
            .within(
                scroll(())
                    .width(percent(100.0))
                    .grow(1.0)
                    .gap(8.0)
                    .padding(UiRect::all(px(pad))),
            ),
        )
    })
}

/// The inspector of `entity`, and below it the card of its material.
fn selection(entity: Entity) -> AnyView<Bevy, EditorTheme> {
    column((entity_inspector(entity), material_slot(entity)))
        .width(percent(100.0))
        .boxed()
}

/// The card for `entity`'s material, when it is one the project owns
/// and so can be edited. Any other is read-only, and gets none.
///
/// Built again when the handle changes, or when an internal asset
/// comes or goes. A rename alone doesn't: the card's own Name row
/// shows it, and rebuilding would take the text field out from under
/// the typing.
fn material_slot(entity: Entity) -> AnyView<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        keyed::<EditorTheme, Option<Uuid>>(
            changing_under(anchor, move |world: &World| {
                internal_material(world, entity)
            }),
            |uuid| match *uuid {
                Some(uuid) => asset_card(
                    AssetId::<StandardMaterial>::Uuid { uuid }
                        .untyped(),
                    "Material".to_string(),
                    Some(Binding::new(InternalName(uuid))),
                )
                .boxed(),
                None => frame().boxed(),
            },
        )
        .within(column(()).width(percent(100.0)))
    })
}

/// The id of `entity`'s material, when the project owns it.
fn internal_material(world: &World, entity: Entity) -> Option<Uuid> {
    let material =
        world.get::<MeshMaterial3d<StandardMaterial>>(entity)?;
    let Handle::Uuid(uuid, _) = material.0 else {
        return None;
    };
    world.resource::<InternalAssets>().get(uuid)?;
    Some(uuid)
}

/// An internal asset's name, for a text field to edit.
#[derive(Clone)]
struct InternalName(Uuid);

impl Source for InternalName {
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>> {
        let asset =
            world.get_resource::<InternalAssets>()?.get(self.0)?;
        Some(Box::new(asset.name.clone()))
    }

    fn set(&self, world: &mut World, value: &dyn PartialReflect) {
        if let Some(name) = String::from_reflect(value) {
            world
                .resource_mut::<InternalAssets>()
                .rename(self.0, name);
        }
    }

    fn changed(
        &self,
    ) -> Box<dyn FnMut(&World) -> bool + Send + Sync> {
        let name = self.clone();
        Box::new(reflect_changed(move |world| name.get(world)))
    }

    fn boxed(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }
}
