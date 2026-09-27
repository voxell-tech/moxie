//! Inspects whatever is selected in the hierarchy: every reflectable
//! component of one entity, each under a collapsible header.

use bevy::asset::uuid::Uuid;
use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use bevy_motiongfx::scene::backend::Backend;
use bevy_motiongfx::scene::id::{EntityUid, SceneUid};
use fynix::composer::Composer;
use fynix::prelude::*;
use motiongfx_scene::block::{Block, Node};
use motiongfx_scene::refs::{FieldRef, TypeName};
use moxie_asset::InternalAssets;
use moxie_ui::elements::{
    EntityInspector, Frame, Label, ScrollArea, asset_card,
};
use moxie_ui::inspector::{Field, Source, reflect_changed};
use moxie_ui::reactive::{
    BevyUi, FynixHost, component_changed_on, resource_changed,
    structure_changed,
};

use crate::SelectedEntity;
use crate::scene::EditorScene;

/// Whether `field` can be animated: it belongs to a scene subject and
/// its [`FieldRef`] is registered in the scene registry. This is what
/// makes an inspector row's label a drag source
/// ([`moxie_ui::inspector::FieldAnimatable`]).
pub(crate) fn is_animatable(world: &World, field: &Field) -> bool {
    let subject = field
        .entity()
        .and_then(|entity| world.get::<EntityUid>(entity));
    if subject.is_none() {
        return false;
    }
    let Some(field_ref) = field_ref_of(world, field) else {
        return false;
    };
    world.get_resource::<EditorScene>().is_some_and(|scene| {
        scene.registry().is_field_registered(&field_ref)
    })
}

/// Whether `field` already drives a [`Node::Action`] somewhere in the
/// scene - what turns its label's diamond blue instead of neutral
/// ([`moxie_ui::inspector::FieldHasAction`]).
pub(crate) fn has_action(world: &World, field: &Field) -> bool {
    let Some(&uid) = field
        .entity()
        .and_then(|entity| world.get::<EntityUid>(entity))
    else {
        return false;
    };
    let Some(field_ref) = field_ref_of(world, field) else {
        return false;
    };
    world.get_resource::<EditorScene>().is_some_and(|scene| {
        block_drives(
            &scene.scene().0.animation,
            SceneUid::Entity(uid),
            &field_ref,
        )
    })
}

/// Whether `block`, or a block nested under it, holds an action for
/// `subject`/`field`.
fn block_drives(
    block: &Block<Backend>,
    subject: SceneUid,
    field: &FieldRef,
) -> bool {
    block.children.iter().any(|child| match child {
        Node::Block { block, .. } => {
            block_drives(block, subject, field)
        }
        Node::Action { action, .. } => {
            action.subject == subject && action.field == *field
        }
        Node::Draft { .. } => false,
    })
}

/// The scene [`FieldRef`] an inspector [`Field`] names - its component's
/// type path plus the reflect path rewritten to the registry's
/// `::`-separated form. `None` for the component root.
pub(crate) fn field_ref_of(
    world: &World,
    field: &Field,
) -> Option<FieldRef> {
    if field.path().is_empty() {
        return None;
    }
    let registry = world.resource::<AppTypeRegistry>().read();
    let type_path =
        registry.get(field.root_type())?.type_info().type_path();
    let path = format!("::{}", field.path().replace('.', "::"));
    Some(FieldRef::new(TypeName::new(type_path), path))
}

/// The inspector panel, as kernel nodes.
pub(super) struct InspectorPanel;

impl Composer<FynixHost> for InspectorPanel {
    type Element = ScrollArea;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, ScrollArea> {
        let pad = ui.theme.space.xl;
        ui.elem(elem!(
            ScrollArea,
            width = percent(100),
            flex_grow = 1.0f32,
            row_gap = px(8),
            padding = UiRect::all(px(pad)),
            scroll_x = false
        ))
        .watch(resource_changed::<SelectedEntity>(), build)
        .handle()
    }
}

fn build(ui: &mut BevyUi) {
    let Some(entity) = ui.world.resource::<SelectedEntity>().0 else {
        let muted = ui.theme.color.text_dim;
        ui.elem(elem!(
            Label,
            text = "Nothing selected",
            color = muted
        ));
        return;
    };
    ui.compose(EntityInspector { entity });

    ui.elem(elem!(
        Frame,
        width = percent(100),
        direction = FlexDirection::Column
    ))
    .watch(material_changed(entity), move |ui| {
        internal_material_card(ui, entity);
    });
}

/// The card for `entity`'s material, when it is one the project owns
/// and so can be edited. Any other is read-only, and gets none.
fn internal_material_card(ui: &mut BevyUi, entity: Entity) {
    let Some(material) = ui
        .world
        .get::<MeshMaterial3d<StandardMaterial>>(entity)
        .map(|material| material.0.clone())
    else {
        return;
    };
    let Handle::Uuid(uuid, _) = material else {
        return;
    };
    if ui.world.resource::<InternalAssets>().get(uuid).is_none() {
        return;
    }
    asset_card(
        ui,
        material.id().untyped(),
        "Material".to_string(),
        Some(&InternalName(uuid)),
    );
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

/// Fires when `entity`'s material handle changes, or when an internal
/// asset comes or goes. A rename alone doesn't: the card's own Name row
/// shows it, and rebuilding would take the text field out from under
/// the typing.
fn material_changed(
    entity: Entity,
) -> impl for<'w> FnMut(WorldNodeRef<'w, FynixHost>) -> bool
+ Send
+ Sync
+ 'static {
    let mut material = component_changed_on::<
        MeshMaterial3d<StandardMaterial>,
    >(entity);
    let mut internal =
        structure_changed::<InternalAssets, _>(|internal| {
            internal.iter().map(|asset| asset.id).collect::<Vec<_>>()
        });
    move |WorldNodeRef { world, node }| {
        let material = material(WorldNodeRef::new(world, node));
        let internal = internal(WorldNodeRef::new(world, node));
        material || internal
    }
}
