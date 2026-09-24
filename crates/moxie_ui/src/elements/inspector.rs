//! Inspectors, as composers.
//!
//! What an inspector is handed (an entity, a component's type, a
//! resource's type) decides what its subtree *is*. There is no patch
//! for "build something else instead", so these read their input
//! once, while building, exactly a [`Composer`]'s window.
//!
//! Each is empty when what it points at is not there. A missing
//! component and an inspector pointed nowhere read the same.

use std::any::TypeId;
use std::borrow::Cow;

use bevy::ecs::reflect::ReflectComponent;
use bevy::prelude::*;
use bevy::reflect::TypeRegistration;
use bevy::reflect::std_traits::ReflectDefault;
use bevy::ui_widgets::{Activate, ActivateOnPress, MenuButton};

use bevy_fynix::WorldEntityMut;
use fynix::composer::Composer;
use fynix::prelude::*;
use fynix::records::{BuildFn, ChangedFn};

use super::button::ButtonCursor;
use super::frame::FrameCursor;
use super::icon::IconCursor;
use super::{
    Dropdown, DropdownItem, DropdownList, DropdownMenu, Frame, Icon,
    Label, TintButton, group_heading, menu_item,
};
use crate::context_menu::context_menu;
use crate::fold::{CHEVRON_OPEN, CHEVRON_SHUT};
use crate::icons;
use crate::inspector::{
    Field, FieldAnimatable, InspectorFields, ReflectEssential,
    ReflectInspectGroup, ReflectInspectable, draggable_field,
    root_leaf, section_open, toggle_section,
};
use crate::reactive::{
    BevyUi, FynixHost, component_changed_on, value_changed,
};
use crate::widgets::tooltip::TooltipExt as _;

/// Inspector for a [`Component`].
pub struct ComponentInspector {
    pub entity: Entity,
    pub component: TypeId,
    /// How many [`Foldable`](crate::fold::Foldable) bodies this sits
    /// under, for `FieldRow` to keep its columns aligned. `0` for
    /// a call site with none of its own.
    pub depth: u32,
}

impl ComponentInspector {
    /// Names the component by type, for a call site that has one
    /// rather than a [`TypeId`].
    pub fn of<T: Component + Reflect>(entity: Entity) -> Self {
        Self {
            entity,
            component: TypeId::of::<T>(),
            depth: 0,
        }
    }
}

impl Composer<FynixHost> for ComponentInspector {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let field = Field::new(self.entity, self.component);
        let built = field.clone();
        let depth = self.depth;

        column(ui, px(4), presence_changed(field), move |ui| {
            if built.exists(ui.world) {
                ui.compose(InspectorFields {
                    root: built.clone(),
                    depth,
                });
            }
        })
    }
}

/// Inspector for a [`Resource`].
pub struct ResourceInspector {
    pub resource: TypeId,
}

impl ResourceInspector {
    /// Names the resource by type, for a call site that has one
    /// rather than a [`TypeId`].
    pub fn of<T: Resource + Reflect>() -> Self {
        Self {
            resource: TypeId::of::<T>(),
        }
    }
}

impl Composer<FynixHost> for ResourceInspector {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let resource = self.resource;

        column(ui, px(4), entity_changed(resource), move |ui| {
            let Some(entity) = resource_entity(ui.world, resource)
            else {
                return;
            };
            ui.compose(InspectorFields {
                root: Field::new(entity, resource),
                depth: 0,
            });
        })
    }
}

/// Inspector for all of the [`Component`]s on an [`Entity`].
pub struct EntityInspector {
    pub entity: Entity,
}

impl Composer<FynixHost> for EntityInspector {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let entity = self.entity;

        column(ui, px(8), components_changed(entity), move |ui| {
            // `None` sorts first, so an ungrouped run never gets
            // mistaken for one under its own (absent) heading.
            let mut shown_group: Option<Option<&'static str>> = None;
            for (component, name, group) in
                inspectable(ui.world, entity)
            {
                if shown_group != Some(group) {
                    shown_group = Some(group);
                    if let Some(group) = group {
                        group_heading(ui, group);
                    }
                }

                component_card(
                    ui,
                    entity,
                    component,
                    name.to_string(),
                );
            }

            ui.compose(AddComponent { entity });
        })
    }
}

/// The menu that adds a component to [`Entity`].
struct AddComponent {
    entity: Entity,
}

impl Composer<FynixHost> for AddComponent {
    type Element = DropdownMenu;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, DropdownMenu> {
        let entity = self.entity;
        let theme = ui.theme;
        let options = addable(ui.world, entity);
        let width = Dropdown::width_for(
            &options
                .iter()
                .map(|(_, name, _)| name.to_string())
                .collect::<Vec<_>>(),
            12.0,
        );

        ui.elem(elem!(DropdownMenu))
            .with(move |ui| {
                let mut add_button = ui.elem(elem!(
                    !TintButton::default(),
                    icon = elem!(
                        Icon,
                        image = icons::PLUS,
                        color = theme.color.text_dim
                    )
                ));
                add_button
                    .insert((MenuButton, ActivateOnPress))
                    .tooltip("Add component");

                ui.elem(elem!(DropdownList, width = width)).with(
                    move |ui| {
                        // A menu popup only opens with a focusable
                        // child, so an empty list says why it's
                        // empty instead of showing nothing.
                        if options.is_empty() {
                            ui.elem(elem!(
                                DropdownItem,
                                label = elem!(
                                    Label,
                                    text = "Nothing left to add"
                                        .to_string(),
                                    color = theme.color.text_dim
                                )
                            ));
                            return;
                        }

                        // `None` sorts first, so an ungrouped run
                        // never gets mistaken for one under its own
                        // (absent) heading.
                        let mut shown_group: Option<
                            Option<&'static str>,
                        > = None;
                        for (component, name, group) in options {
                            if shown_group != Some(group) {
                                shown_group = Some(group);
                                if let Some(group) = group {
                                    group_heading(ui, group);
                                }
                            }
                            add_component_item(
                                ui, entity, component, &name,
                            );
                        }
                    },
                );
            })
            .handle()
    }
}

/// One entry in [`AddComponent`]'s list. Picking it inserts the
/// component and closes the list.
fn add_component_item(
    ui: &mut BevyUi,
    entity: Entity,
    component: TypeId,
    name: &str,
) {
    menu_item(ui, None, name, move |world| {
        add_component(world, entity, component);
    });
}

/// Every [`register_inspectable`](
/// crate::inspector::InspectAppExt::register_inspectable) type
/// `entity` does not already carry, with a registered
/// [`ReflectDefault`] to construct one with.
///
/// Sorted by [`with_inspect_group`](
/// crate::inspector::InspectAppExt::with_inspect_group)'s group
/// (ungrouped first), then by name within it, for the same reason
/// [`inspectable`] sorts by name.
fn addable(
    world: &World,
    entity: Entity,
) -> Vec<(TypeId, Cow<'static, str>, Option<&'static str>)> {
    let Ok(entity_ref) = world.get_entity(entity) else {
        return Vec::new();
    };

    let registry = world.resource::<AppTypeRegistry>().read();
    let mut out: Vec<(
        TypeId,
        Cow<'static, str>,
        Option<&'static str>,
    )> = registry
        .iter()
        .filter(|registration| {
            registration.data::<ReflectInspectable>().is_some()
                && registration.data::<ReflectDefault>().is_some()
        })
        .filter_map(|registration| {
            let reflect_component =
                registration.data::<ReflectComponent>()?;
            if reflect_component.contains(entity_ref) {
                return None;
            }
            let group = registration
                .data::<ReflectInspectGroup>()
                .map(|group| group.0);
            Some((
                registration.type_id(),
                display_name(registration),
                group,
            ))
        })
        .collect();

    out.sort_by(|(_, a_name, a_group), (_, b_name, b_group)| {
        a_group.cmp(b_group).then_with(|| a_name.cmp(b_name))
    });
    out
}

/// Inserts `component`'s default value onto `entity`. Does nothing
/// if the entity despawned or the component arrived some other way
/// before this runs.
fn add_component(
    world: &mut World,
    entity: Entity,
    component: TypeId,
) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let Some(registration) = registry.get(component) else {
        return;
    };
    let (Some(default), Some(reflect_component)) = (
        registration.data::<ReflectDefault>(),
        registration.data::<ReflectComponent>(),
    ) else {
        return;
    };
    let Ok(mut entity) = world.get_entity_mut(entity) else {
        return;
    };
    if reflect_component.contains(entity.as_readonly()) {
        return;
    }

    let value = default.default();
    reflect_component.insert(
        &mut entity,
        value.as_partial_reflect(),
        &registry,
    );
}

/// Removes `component` from `entity`. Does nothing if the entity
/// despawned, never carried it, or its type isn't registered.
fn remove_component(
    world: &mut World,
    entity: Entity,
    component: TypeId,
) {
    if essential(world, component) {
        return;
    }

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let Some(reflect_component) =
        registry.get(component).and_then(|registration| {
            registration.data::<ReflectComponent>()
        })
    else {
        return;
    };
    let Ok(mut entity) = world.get_entity_mut(entity) else {
        return;
    };
    reflect_component.remove(&mut entity);
}

/// Whether `component` was opted in via
/// [`register_essential`](crate::inspector::InspectAppExt::register_essential).
fn essential(world: &World, component: TypeId) -> bool {
    let registry = world.resource::<AppTypeRegistry>().read();
    registry.get(component).is_some_and(|registration| {
        registration.data::<ReflectEssential>().is_some()
    })
}

/// A card's own fold state, distinct from the shared nested-fold
/// hierarchy `crate::fold` builds: a component's title bar is not
/// another level of that, just a bar on top of its card, so its body
/// sits flush with no rail or indent under it.
#[derive(Component)]
struct CardClosed;

/// One component's own card: a title that's always there, above a
/// body that folds flush under it - no rail, no indent, each field
/// reading like its own root - like Unity's per-component panel.
fn component_card(
    ui: &mut BevyUi,
    entity: Entity,
    component: TypeId,
    name: String,
) {
    let deletable = !essential(ui.world, component);
    let open = section_open(ui.world, entity, component, "");
    // The title stands in for a genuine field's own name when the
    // whole component is one nameless leaf, so it carries that
    // field's drag source too, same as `FieldName` gives a row of
    // its own.
    let drag_field =
        root_leaf(ui.world, &Field::new(entity, component)).filter(
            |field| {
                ui.world
                    .resource::<FieldAnimatable>()
                    .allows(ui.world, field)
            },
        );
    let background = ui.theme.color.panel;
    let radius = ui.theme.space.card_radius;
    let padding = ui.theme.space.card_padding;
    let muted = ui.theme.color.text_dim;
    let primary = ui.theme.color.text;
    let title = name.clone();

    let mut card = ui.elem(elem!(
        Frame,
        width = percent(100),
        direction = FlexDirection::Column,
        background = background,
        radius = px(radius),
        padding = UiRect::all(px(padding)),
        overflow = Overflow::clip()
    ));
    let node = card.id();
    if !open {
        card.insert(CardClosed);
    }

    card.with(move |ui| {
        let mut header = ui.elem(elem!(
            !TintButton::default(),
            width = percent(100),
            justify = JustifyContent::FlexStart,
            icon = elem!(
                Icon,
                image = icons::CHEVRON,
                color = muted,
                rotation =
                    if open { CHEVRON_OPEN } else { CHEVRON_SHUT }
            ),
            label = elem!(
                Label,
                text = title,
                color = primary,
                bold = true
            )
        ));
        header
            .observe(
                move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |world: &mut World| {
                        let opening =
                            world.get::<CardClosed>(node).is_some();
                        if let Ok(mut card) =
                            world.get_entity_mut(node)
                        {
                            if opening {
                                card.remove::<CardClosed>();
                            } else {
                                card.insert(CardClosed);
                            }
                        }
                        toggle_section(
                            world,
                            entity,
                            component,
                            String::new(),
                            opening,
                        );
                    });
                },
            )
            .bind(
                |button| button.icon().rotation(),
                component_changed_on::<CardClosed>(node),
                move |WorldNodeRef { world, .. }| {
                    if world.get::<CardClosed>(node).is_some() {
                        CHEVRON_SHUT
                    } else {
                        CHEVRON_OPEN
                    }
                },
            );

        if let Some(field) = drag_field.clone() {
            draggable_field(&mut header, field, name);
        }

        if deletable {
            context_menu(&mut header, move |menu| {
                let critical = menu.theme().color.critical;
                menu.item(
                    Some((icons::TRASH, critical)),
                    "Delete",
                    move |world| {
                        remove_component(world, entity, component);
                    },
                );
            });
        }

        ui.elem(elem!(
            Frame,
            width = percent(100),
            direction = FlexDirection::Column
        ))
        .bind(
            |frame| frame.display(),
            component_changed_on::<CardClosed>(node),
            move |WorldNodeRef { world, .. }| {
                if world.get::<CardClosed>(node).is_some() {
                    Display::None
                } else {
                    Display::Flex
                }
            },
        )
        .watch(
            component_changed_on::<CardClosed>(node),
            move |ui| {
                if ui.world.get::<CardClosed>(node).is_some() {
                    return;
                }
                ui.compose(ComponentInspector {
                    entity,
                    component,
                    depth: 0,
                });
            },
        );
    });
}

/// The entity bevy is currently keeping `resource` on.
fn resource_entity(
    world: &World,
    resource: TypeId,
) -> Option<Entity> {
    let id = world.components().get_id(resource)?;
    world.resource_entities().get(id)
}

/// Fires when the entity holding `resource` changes, and on the
/// first poll.
///
/// Only moves when the resource is removed and re-inserted: a
/// different entity means the subtree should rebuild.
fn entity_changed(
    resource: TypeId,
) -> impl for<'w> FnMut(WorldNodeRef<'w, FynixHost>) -> bool {
    value_changed(move |world, _| resource_entity(world, resource))
}

/// Fires when `entity`'s set of inspectable components changes, and
/// on the first poll.
///
/// A component's *value* is each section's own business. This only
/// rebuilds when one is added, removed, or the entity goes.
fn components_changed(
    entity: Entity,
) -> impl for<'w> FnMut(WorldNodeRef<'w, FynixHost>) -> bool {
    value_changed(move |world, _| inspectable(world, entity))
}

/// Every component on `entity` the inspector can reach and shows, by
/// type, the name its section is headed with, and its
/// [`with_inspect_group`](
/// crate::inspector::InspectAppExt::with_inspect_group) group.
///
/// Sorted by group (ungrouped first), then by name within it: an
/// archetype lists what it holds in whatever order it happens to, and
/// a panel whose sections reshuffle when a component is added is no
/// use to read.
fn inspectable(
    world: &World,
    entity: Entity,
) -> Vec<(TypeId, Cow<'static, str>, Option<&'static str>)> {
    let Ok(components) = world.inspect_entity(entity) else {
        return Vec::new();
    };
    // Collected before the registry is read: both borrow the world.
    let ids: Vec<TypeId> =
        components.filter_map(|info| info.type_id()).collect();

    let registry = world.resource::<AppTypeRegistry>().read();
    let mut out: Vec<(
        TypeId,
        Cow<'static, str>,
        Option<&'static str>,
    )> = ids
        .into_iter()
        .filter_map(|id| {
            let registration = registry.get(id)?;
            // Without this there is no way to reach the value at
            // all, whatever its fields would have said.
            registration.data::<ReflectComponent>()?;
            // Opt-in - see InspectAppExt::register_inspectable.
            registration.data::<ReflectInspectable>()?;
            let group = registration
                .data::<ReflectInspectGroup>()
                .map(|group| group.0);
            Some((id, display_name(registration), group))
        })
        .collect();

    out.sort_by(|(_, a_name, a_group), (_, b_name, b_group)| {
        a_group.cmp(b_group).then_with(|| a_name.cmp(b_name))
    });
    out
}

/// What [`ReflectInspectable::name`] overrides to, or `T`'s own name
/// split into words.
pub fn display_name(
    registration: &TypeRegistration,
) -> Cow<'static, str> {
    if let Some(name) = registration
        .data::<ReflectInspectable>()
        .and_then(|inspectable| inspectable.name)
    {
        return Cow::Borrowed(name);
    }

    Cow::Owned(humanize(
        registration.type_info().type_path_table().short_path(),
    ))
}

/// `name` split at each lowercase-to-uppercase or letter-to-digit
/// boundary, so a type's own Rust-cased name reads as words.
fn humanize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev: Option<char> = None;

    for ch in name.chars() {
        let splits = prev.is_some_and(|prev| {
            (prev.is_lowercase() && ch.is_uppercase())
                || (prev.is_alphabetic() && ch.is_numeric())
        });
        if splits {
            out.push(' ');
        }
        out.push(ch);
        prev = Some(ch);
    }

    out
}

/// The column an inspector fills, and the watcher that fills it.
///
/// `gap` is what separates its rows: wider between whole components
/// than between the fields of one.
fn column(
    ui: &mut BevyUi,
    gap: Val,
    changed: impl ChangedFn<FynixHost>,
    build: impl BuildFn<FynixHost>,
) -> ElementHandle<FynixHost, Frame> {
    ui.elem(elem!(
        Frame,
        width = percent(100),
        direction = FlexDirection::Column,
        row_gap = gap
    ))
    .watch(changed, build)
    .handle()
}

/// Fires when `field`'s component appears or disappears, and on the
/// first poll.
///
/// Presence only. What the component holds is
/// [`InspectorFields`]' own business, with its own watcher for that.
fn presence_changed(
    field: Field,
) -> impl for<'w> FnMut(WorldNodeRef<'w, FynixHost>) -> bool {
    value_changed(move |world, _| field.exists(world))
}
