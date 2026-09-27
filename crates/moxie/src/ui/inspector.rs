//! Inspects whatever is selected in the hierarchy: every reflectable
//! component of one entity, each under a collapsible header.

use bevy::asset::uuid::Uuid;
use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use fynix::composer::Composer;
use fynix::prelude::*;
use moxie_asset::InternalAssets;
use moxie_ui::elements::{
    EntityInspector, Frame, Label, ScrollArea, asset_card,
};
use moxie_ui::inspector::{Source, reflect_changed};
use moxie_ui::reactive::{
    BevyUi, FynixHost, component_changed_on, either,
    resource_changed, structure_changed,
};

use crate::SelectedEntity;

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
    either(
        component_changed_on::<MeshMaterial3d<StandardMaterial>>(
            entity,
        ),
        structure_changed::<InternalAssets, _>(|internal| {
            internal.iter().map(|asset| asset.id).collect::<Vec<_>>()
        }),
    )
}
