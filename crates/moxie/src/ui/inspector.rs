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
use moxie_ui::elements::{EntityInspector, Label, ScrollArea};
use moxie_ui::inspector::{Field, Source, reflect_changed};
use moxie_ui::reactive::{BevyUi, FynixHost, resource_changed};

use crate::SelectedEntity;
use crate::scene::EditorScene;

/// Whether `field` can be animated: it belongs to a scene subject and
/// its [`FieldRef`] is registered in the scene registry. This is what
/// makes an inspector row's label a drag source
/// ([`moxie_ui::inspector::FieldAnimatable`]).
pub(crate) fn is_animatable(world: &World, field: &Field) -> bool {
    if world.get::<EntityUid>(field.entity()).is_none() {
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
    let Some(uid) = world.get::<EntityUid>(field.entity()).copied()
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
        registry.get(field.component())?.type_info().type_path();
    let path = format!("::{}", field.path().replace('.', "::"));
    Some(FieldRef::new(TypeName::new(type_path), path))
}

/// `field`'s own stage entry, if it has one; see
/// [`moxie_ui::inspector::FieldStageSource`].
pub(crate) fn stage_source(
    world: &World,
    field: &Field,
) -> Option<Box<dyn Source>> {
    let uid = world.get::<EntityUid>(field.entity()).copied()?;
    let field_ref = field_ref_of(world, field)?;
    let staged = StagedField {
        subject: SceneUid::Entity(uid),
        field: field_ref,
    };
    staged.seed_id(world)?;
    Some(Box::new(staged))
}

/// A field's own entry in the scene's
/// [`Stage`](motiongfx_scene::scene::Stage).
#[derive(Clone)]
struct StagedField {
    subject: SceneUid,
    field: FieldRef,
}

impl StagedField {
    fn seed_id(&self, world: &World) -> Option<Uuid> {
        let scene = world.get_resource::<EditorScene>()?.scene();
        scene
            .0
            .stage
            .subjects
            .iter()
            .find(|subject| subject.id == self.subject)?
            .fields
            .iter()
            .find(|seed| seed.field == self.field)
            .map(|seed| seed.value)
    }
}

impl Source for StagedField {
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>> {
        let id = self.seed_id(world)?;
        let values =
            &world.get_resource::<EditorScene>()?.scene().0.values;

        values
            .f32
            .get(&id)
            .map(|value| Box::new(*value) as Box<dyn PartialReflect>)
            .or_else(|| {
                values.vec3.get(&id).map(|value| {
                    Box::new(*value) as Box<dyn PartialReflect>
                })
            })
            .or_else(|| {
                values.quat.get(&id).map(|value| {
                    Box::new(*value) as Box<dyn PartialReflect>
                })
            })
    }

    fn set(&self, world: &mut World, value: &dyn PartialReflect) {
        let Some(id) = self.seed_id(world) else {
            return;
        };
        let Some(mut editor) =
            world.get_resource_mut::<EditorScene>()
        else {
            return;
        };
        let values = &mut editor.edit().values;

        if let Some(slot) = values.f32.get_mut(&id) {
            let _ = slot.try_apply(value);
        } else if let Some(slot) = values.vec3.get_mut(&id) {
            let _ = slot.try_apply(value);
        } else if let Some(slot) = values.quat.get_mut(&id) {
            let _ = slot.try_apply(value);
        }
    }

    fn changed(
        &self,
    ) -> Box<dyn FnMut(&World) -> bool + Send + Sync> {
        let staged = self.clone();
        Box::new(reflect_changed(move |world| staged.get(world)))
    }

    fn boxed(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }
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
}
