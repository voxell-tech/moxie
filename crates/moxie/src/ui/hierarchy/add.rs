//! The hierarchy's add menu: one row per kind of subject, each
//! spawning one at the top level.

use bevy::prelude::*;
use bevy::ui_widgets::popover::{
    Popover, PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy::ui_widgets::{ActivateOnPress, MenuButton};
use bevy_fynix::WorldEntityMut;
use bevy_motiongfx::scene::id::EntityUid;
use fynix::composer::Composer;
use fynix::prelude::*;
use moxie_ui::elements::{
    DropdownList, DropdownMenu, Frame, Icon, TintButton,
    group_heading, menu_item,
};
use moxie_ui::inspector::ReflectEssential;
use moxie_ui::reactive::{BevyUi, FynixHost};
use moxie_ui::widgets::tooltip::TooltipExt as _;

use crate::shape::{Shape2d, Shape2dKind};
use crate::{SceneRoot, SelectedEntity};

/// The material every mesh kind is spawned with.
const MATERIAL: &str = "materials/default.mat";

/// The menu of kinds a subject can be spawned as.
pub(super) struct AddMenu;

impl Composer<FynixHost> for AddMenu {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let pad = ui.theme.space.xl;

        ui.elem(elem!(
            Frame,
            position = PositionType::Absolute,
            inset = UiRect::new(auto(), px(pad), auto(), px(pad))
        ))
        .with(|ui| {
            ui.elem(elem!(DropdownMenu)).with(|ui| {
                let mut button = ui.elem(elem!(
                    !TintButton::default(),
                    icon = elem!(Icon, image = crate::icons::PLUS)
                ));
                button
                    .insert((MenuButton, ActivateOnPress))
                    .tooltip("New entity");

                let margin = ui.theme.space.menu_margin;
                ui.elem(elem!(DropdownList))
                    .insert(Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Top,
                                align: PopoverAlign::End,
                                gap: 2.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::End,
                                gap: 2.0,
                            },
                        ],
                        window_margin: margin,
                    })
                    .with(rows);
            });
        })
        .handle()
    }
}

/// The components a menu row adds, on top of the essentials.
#[derive(Clone, Copy)]
enum Spawn {
    Empty,
    Shape(Shape2dKind),
    /// The mesh asset under this path.
    Mesh(&'static str),
    Text,
}

/// The menu, in the order it reads.
fn rows(ui: &mut BevyUi) {
    item(ui, "Empty", Spawn::Empty);

    group_heading(ui, "2D");
    item(ui, "Circle", Spawn::Shape(Shape2dKind::Circle));
    item(ui, "Rectangle", Spawn::Shape(Shape2dKind::Rectangle));
    item(ui, "Triangle", Spawn::Shape(Shape2dKind::Triangle));
    item(ui, "Text", Spawn::Text);

    group_heading(ui, "3D");
    item(ui, "Cube", Spawn::Mesh("meshes/cube.glb"));
    item(ui, "Sphere", Spawn::Mesh("meshes/sphere.glb"));
    item(ui, "Plane", Spawn::Mesh("meshes/plane.glb"));
    item(ui, "Cylinder", Spawn::Mesh("meshes/cylinder.glb"));
    item(ui, "Cone", Spawn::Mesh("meshes/cone.glb"));
    item(ui, "Torus", Spawn::Mesh("meshes/torus.glb"));
}

/// A row that spawns `spawn`, named `name`.
fn item(ui: &mut BevyUi, name: &'static str, spawn: Spawn) {
    menu_item(ui, None, name, move |world| add(world, name, spawn));
}

/// Spawns a subject at the top level, names it `name`, and selects it
/// so the inspector is already pointed at what was just made.
fn add(world: &mut World, name: &'static str, spawn: Spawn) {
    let Ok(root) = world
        .query_filtered::<Entity, With<SceneRoot>>()
        .single(world)
    else {
        return;
    };

    let id = world.spawn((EntityUid::new(), ChildOf(root))).id();
    insert_essential(world, id);

    let mut entity = world.entity_mut(id);
    entity.insert(Name::new(name));
    match spawn {
        Spawn::Empty => {}
        Spawn::Shape(kind) => {
            entity.insert(Shape2d { kind, ..default() });
        }
        Spawn::Mesh(path) => {
            let assets = entity.resource::<AssetServer>();
            let mesh = assets.load(
                GltfAssetLabel::Primitive {
                    mesh: 0,
                    primitive: 0,
                }
                .from_asset(path),
            );
            let material = assets.load::<StandardMaterial>(MATERIAL);
            entity.insert((Mesh3d(mesh), MeshMaterial3d(material)));
        }
        Spawn::Text => {
            entity.insert(Text2d::new("Text"));
        }
    }

    world.insert_resource(SelectedEntity(Some(id)));
}

/// Inserts every [`register_essential`](
/// moxie_ui::inspector::InspectAppExt::register_essential)
/// component onto `entity`, holding the value it spawns with.
fn insert_essential(world: &mut World, entity: Entity) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let essentials = registry
        .iter()
        .filter_map(|registration| {
            Some((
                registration.data::<ReflectComponent>()?,
                registration.data::<ReflectEssential>()?.spawn(),
            ))
        })
        .collect::<Vec<_>>();

    let Ok(mut entity) = world.get_entity_mut(entity) else {
        return;
    };
    for (reflect_component, value) in &essentials {
        reflect_component.insert(
            &mut entity,
            value.as_partial_reflect(),
            &registry,
        );
    }
}
