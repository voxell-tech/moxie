//! Inspects whatever is selected in the hierarchy: every reflectable
//! component of one entity, each under a collapsible header.

use bevy::prelude::*;
use bevy_motiongfx::scene::id::EntityUid;
use fynix::composer::Composer;
use fynix::prelude::*;
use motiongfx_scene::refs::{FieldRef, TypeName};
use moxie_ui::elements::{EntityInspector, Label, ScrollArea};
use moxie_ui::inspector::Field;
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
