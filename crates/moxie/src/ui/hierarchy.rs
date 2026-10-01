//! Scene hierarchy browser: an indented list of the scene's subjects.
//!
//! What counts as one is an [`EntityUid`], the id a scene refers to
//! its subjects by. The panel lists exactly what the animation can
//! address, nothing the editor spawned for itself.
//!
//! Each subject watches only its own children, so adding one builds
//! just the new row. Depth is the nesting: a subtree indents what it
//! holds.

mod drag;
#[cfg(test)]
mod tests;

use bevy::asset::AssetServer;
use bevy::camera::NormalizedRenderTarget;
use bevy::ecs::query::QueryState;
use bevy::ecs::reflect::ReflectComponent;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::{
    Location, PointerButton, PointerId, PointerLocation,
};
use bevy::prelude::*;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, ContextMenuExt as _, FrameProps as _,
    TooltipExt as _, button, column, ghost, icon, label, menu_item,
    row, scroll, tint,
};
use bevy_fynix::{
    AnyView, Bevy, ScopedExt as _, Signal, View, ViewExt as _,
    component, each, keyed,
};
use bevy_motiongfx::scene::id::EntityUid;
pub(crate) use drag::Dragging;
use moxie_ui::fold::{Chevron, Foldable, FoldsOn};
use moxie_ui::gaps::{anchored, changing, changing_under};
use moxie_ui::inspector::ReflectEssential;
use moxie_ui::theme::EditorTheme;

use self::drag::{At, below, rows};
use crate::subject::Caption;
use crate::{SceneRoot, SelectedEntity, presets};

/// The [`tail`]'s least height: room below the last row for the
/// floating button, and a drop target even when the list is full.
const TAIL_MIN: f32 = 34.0;

/// A row's height.
const ROW_HEIGHT: f32 = 18.0;

/// The panel: every scene subject as nested rows, with the one thing
/// that acts on the list as a whole floated over its corner.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        let plus = cx
            .world
            .resource::<AssetServer>()
            .load(crate::icons::PLUS);
        cx.build(
            column((roots_list(pad), add_button(plus, pad)))
                .width(percent(100.0))
                .height(percent(100.0))
                .gap(0.0),
        )
    })
}

/// The subjects themselves, scrolling under the button, closed off by
/// the [`tail`].
fn roots_list(pad: f32) -> impl View<Bevy, EditorTheme> {
    scroll((
        each::<EditorTheme, Entity, Entity>(
            roots_signal(),
            |root| *root,
            |root| subtree(*root),
        )
        .within(column(()).width(percent(100.0)).gap(0.0)),
        tail(),
    ))
    .width(percent(100.0))
    .grow(1.0)
    .gap(0.0)
    .padding(UiRect::all(px(pad)))
}

/// The top-level subjects, re-read when they change.
///
/// Roots only: a branch minds itself. The query is kept because the
/// check runs every update, and the read, which makes its own, far
/// less often.
fn roots_signal() -> Signal<Vec<Entity>> {
    let mut query = None::<QueryState<Entity, With<SceneRoot>>>;
    let mut seen = None::<Vec<Entity>>;

    Signal::new(
        |world: &World| {
            QueryState::try_new(world)
                .map(|mut query| roots(world, &mut query))
                .unwrap_or_default()
        },
        move |world: &World| {
            let query = match &mut query {
                Some(query) => query,
                slot => match QueryState::try_new(world) {
                    Some(query) => slot.insert(query),
                    None => return false,
                },
            };
            query.update_archetypes(world);

            let current = roots(world, query);
            let changed = seen.as_ref() != Some(&current);
            seen = Some(current);
            changed
        },
    )
}

/// The strip below the last root row, filling whatever height is
/// left. A drop anywhere on it lands the row at the top level after
/// the last one, clear of any open branch.
fn tail() -> impl View<Bevy, EditorTheme> {
    below(
        column(())
            .width(percent(100.0))
            .grow(1.0)
            .min_height(px(TAIL_MIN)),
    )
}

/// The one thing that acts on the list.
///
/// Floated over the corner, so it stays put however far the list is
/// scrolled. Pressing it opens the add menu where the pointer is.
fn add_button(
    plus: Handle<Image>,
    pad: f32,
) -> impl View<Bevy, EditorTheme> {
    button(icon(plus).tone(Tone::Accent))
        .position(PositionType::Absolute)
        .inset(UiRect::new(Val::Auto, px(pad), Val::Auto, px(pad)))
        .padding(UiRect::all(px(4.0)))
        .rules(tint)
        .on_activate_with(open_menu_at_pointer)
        .context_menu(add_menu)
        .tooltip(|| label("Add"))
        .tagged(Name::new("Add"))
}

/// Opens the context menu of `node` as a right press there would, at
/// the pointer. Pressed with no pointer, from the keyboard, it opens
/// at the corner, where placement pushes it on screen.
fn open_menu_at_pointer(world: &mut World, node: Entity) {
    let location = world
        .query::<&PointerLocation>()
        .iter(world)
        .find_map(|pointer| pointer.location().cloned())
        .unwrap_or(Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        });
    world.trigger(Pointer::new(
        PointerId::Mouse,
        location,
        Press {
            button: PointerButton::Secondary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        },
        node,
    ));
}

/// What the add button offers: an empty subject, or one that already
/// shows something.
fn add_menu() -> Vec<AnyView<Bevy, EditorTheme>> {
    let entry = |name: &'static str,
                 add: fn(&mut World, &'static str)| {
        menu_item(label(name).wrap(false))
            .on_activate(move |world| add(world, name))
            .boxed()
    };

    let mut rows = vec![entry("Empty", |world, _| {
        spawn_new_entity(world);
    })];
    rows.extend(
        ADD_MESHES.iter().map(|&name| entry(name, spawn_mesh)),
    );
    rows.push(entry("Point Light", |world, name| {
        spawn_named(world, name, PointLight::default());
    }));
    rows.push(entry("Directional Light", |world, name| {
        spawn_named(world, name, DirectionalLight::default());
    }));
    rows
}

/// The seam on one side of a row, and the line a drop lights on it.
/// Full width of the list, so the line is exactly as wide as the
/// rows.
///
/// A drop after one row and a drop before the next land in the same
/// place, so either lights the line there. The line is out of the
/// pointer's way, since it overlaps the edges of the rows it sits
/// between, and those are what aim the drop.
fn seam(entity: Entity, at: At) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let theme = cx.theme();
        let accent = theme.color.accent;
        let thickness = theme.space.edge;
        let lit = changing(move |world: &World| {
            world.resource::<Dragging>().shows(entity, at)
        })
        .map(move |lit| if lit { accent } else { Color::NONE });

        cx.build(
            column((column(())
                .width(percent(100.0))
                .height(px(thickness))
                // Or the line is squeezed out of the seam's height.
                .shrink(0.0)
                .fill(lit)
                .with(Pickable::IGNORE),))
            .width(percent(100.0))
            // No height, and the line overflows it: the seam then
            // adds nothing to the list, and the indent rails, which
            // stretch to the list's height, are not dragged past
            // its last row.
            .height(px(0.0))
            // Or a flex item's auto floor grows it back to the
            // line's own height.
            .min_height(px(0.0))
            .shrink(0.0)
            .justify(JustifyContent::Center)
            .gap(0.0)
            .with(Pickable::IGNORE),
        )
    })
}

/// One subject, and everything under it, between the two seams a drop
/// next to it lights.
fn subtree(entity: Entity) -> AnyView<Bevy, EditorTheme> {
    column((
        seam(entity, At::Before),
        branch(entity),
        seam(entity, At::After),
    ))
    .width(percent(100.0))
    .gap(0.0)
    .boxed()
}

/// A subject's row, built again when it gains its first child or
/// loses its last, which is what decides whether it folds.
fn branch(entity: Entity) -> AnyView<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        keyed::<EditorTheme, bool>(
            changing_under(anchor, move |world: &World| {
                has_children(world, entity)
            }),
            move |&enabled| fold(entity, enabled),
        )
        .within(column(()).width(percent(100.0)).gap(0.0))
    })
}

/// The subject's row over the subjects under it.
fn fold(entity: Entity, enabled: bool) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let trash = cx
            .world
            .resource::<AssetServer>()
            .load(moxie_ui::icons::TRASH);
        // Read off the subject's own entity. A row is built fresh
        // when it moves to another parent, but the entity, and
        // `Collapsed` on it, is not. Nothing to clean up when a
        // subject is deleted either: `Collapsed` goes with it.
        let open = cx.world.get::<Collapsed>(entity).is_none();
        cx.build(
            Foldable::new(
                move |_: Chevron| header(entity, enabled, trash),
                move || children(entity),
            )
            // The row is the subject's, to select; only the chevron
            // beside it folds.
            .folds_on(FoldsOn::Chevron)
            .enabled(enabled)
            .open(open)
            .on_toggle(
                move |world: &mut World, open: bool| {
                    let Ok(mut entity) = world.get_entity_mut(entity)
                    else {
                        return;
                    };
                    if open {
                        entity.remove::<Collapsed>();
                    } else {
                        entity.insert(Collapsed);
                    }
                },
            ),
        )
    })
}

/// The subjects directly under `entity`, one subtree each.
fn children(entity: Entity) -> AnyView<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        each::<EditorTheme, Entity, Entity>(
            changing_under(anchor, move |world: &World| {
                children_of(world, entity)
            }),
            |child| *child,
            |child| subtree(*child),
        )
        .within(column(()).width(percent(100.0)).gap(0.0))
    })
}

/// The button that selects `entity`, can be picked up and dropped on,
/// and has a menu of its own.
///
/// A leaf, with no chevron before it, is set in by the width of one
/// so its name lines up with its siblings'.
fn header(
    entity: Entity,
    enabled: bool,
    trash: Handle<Image>,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let theme = cx.theme();
        let accent = theme.color.accent;
        let selected = theme.color.selection;
        let toggle = theme.space.fold_toggle;
        let trash = trash.clone();
        let uid = cx.world.get::<EntityUid>(entity).copied();

        let text = component::<Name, _>(entity, move |name| {
            caption(name, uid).text().to_string()
        });
        let tone = component::<Name, _>(entity, move |name| {
            if caption(name, uid).name.is_some() {
                Tone::Body
            } else {
                Tone::Dim
            }
        });
        // One signal: a second on the same fill would fight this one
        // every update.
        let fill = changing(move |world: &World| {
            (
                world.resource::<SelectedEntity>().0 == Some(entity),
                world.resource::<Dragging>().shows(entity, At::Into),
            )
        })
        .map(move |(is_selected, into)| {
            // A drop landing inside beats whether it is selected, and
            // most rows are neither.
            if into {
                accent.with_alpha(0.35)
            } else if is_selected {
                selected
            } else {
                Color::NONE
            }
        });

        cx.build(rows(
            button(label(text).tone(tone).wrap(false))
                .grow(1.0)
                .height(px(ROW_HEIGHT))
                .justify(JustifyContent::FlexStart)
                .margin(UiRect::left(px(if enabled {
                    0.0
                } else {
                    toggle
                })))
                .padding(UiRect::axes(px(4.0), Val::ZERO))
                .radius(3.0)
                .fill(fill)
                .rules(ghost)
                .on_activate(move |world| {
                    world.resource_mut::<SelectedEntity>().0 =
                        Some(entity);
                })
                .context_menu(move || {
                    (menu_item(
                        row((
                            icon(trash.clone()).tone(Tone::Critical),
                            label("Delete").tone(Tone::Critical),
                        ))
                        .align(AlignItems::Center),
                    )
                    .on_activate(move |world| {
                        despawn_entity(world, entity);
                    }),)
                }),
            entity,
        ))
    })
}

/// On a subject's own entity while its hierarchy row is collapsed.
/// Nothing removes this when the row's own node is despawned and
/// rebuilt, since it was never on that node. It goes when the
/// subject itself does.
#[derive(Component)]
struct Collapsed;

/// Spawns a subject at the top level, and selects it so the inspector
/// is already pointed at what was just made.
///
/// Nothing of the animation changes: a
/// [`Stage`](motiongfx_scene::scene::Stage) seeds the fields an
/// action drives, and a subject with no action on it keeps whatever
/// it was spawned holding.
fn spawn_new_entity(world: &mut World) -> Option<Entity> {
    let Ok(root) = world
        .query_filtered::<Entity, With<SceneRoot>>()
        .single(world)
    else {
        error!("Scene root does not exist!");
        return None;
    };

    let entity = world.spawn((EntityUid::new(), ChildOf(root))).id();
    insert_essential(world, entity);

    world.insert_resource(SelectedEntity(Some(entity)));
    Some(entity)
}

/// Meshes the add menu offers, by their [`presets::MESHES`] name.
const ADD_MESHES: &[&str] = &[
    "Cube", "Sphere", "Plane", "Cylinder", "Cone", "Torus", "Monkey",
];

/// Spawns the mesh preset `name`, selected.
pub(crate) fn spawn_mesh(world: &mut World, name: &str) {
    let Some(&(_, path)) =
        presets::MESHES.iter().find(|(preset, _)| *preset == name)
    else {
        return;
    };
    let visual = (
        Mesh3d(world.resource::<AssetServer>().load(path)),
        MeshMaterial3d(presets::DEFAULT_MATERIAL),
    );
    spawn_named(world, name, visual);
}

/// A [`spawn_new_entity`] named `name` and holding `bundle`.
fn spawn_named(world: &mut World, name: &str, bundle: impl Bundle) {
    if let Some(entity) = spawn_new_entity(world) {
        world
            .entity_mut(entity)
            .insert((Name::new(name.to_string()), bundle));
    }
}

/// Inserts every [`register_essential`](
/// moxie_ui::inspector::InspectAppExt::register_essential)
/// component's default value onto `entity` - `Name`, `Transform`,
/// `Visibility` today, whatever the registry answers for tomorrow.
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

/// Despawns `entity` and everything under it, and clears the
/// selection if it pointed there.
fn despawn_entity(world: &mut World, entity: Entity) {
    // Ahead of the despawn: `world.despawn` takes the whole subtree
    // with it, so a selected descendant is no longer reachable to
    // check for afterward.
    let selected = world.resource::<SelectedEntity>().0;
    let clears = selected
        .is_some_and(|selected| under(world, entity, selected));

    world.despawn(entity);

    if clears {
        world.resource_mut::<SelectedEntity>().0 = None;
    }
}

/// Whether `entity` is `ancestor` itself or sits somewhere under it.
fn under(world: &World, ancestor: Entity, entity: Entity) -> bool {
    if ancestor == entity {
        return true;
    }
    world.get::<Children>(ancestor).is_some_and(|children| {
        children.iter().any(|child| under(world, child, entity))
    })
}

/// The top-level subjects, in the order [`SceneRoot`] holds them.
///
/// The root itself is never a row: it exists to give the top level an
/// order.
fn roots(
    world: &World,
    query: &mut QueryState<Entity, With<SceneRoot>>,
) -> Vec<Entity> {
    query
        .iter_manual(world)
        .next()
        .map(|root| children_of(world, root))
        .unwrap_or_default()
}

/// The subjects directly under `entity`; anything else it parents is
/// not the scene's to show.
fn children_of(world: &World, entity: Entity) -> Vec<Entity> {
    world
        .get::<Children>(entity)
        .map(|children| {
            children
                .iter()
                .filter(|&child| is_subject(world, child))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `entity` has any subject under it, without collecting
/// them. A row only needs to know there is something to fold.
fn has_children(world: &World, entity: Entity) -> bool {
    world.get::<Children>(entity).is_some_and(|children| {
        children.iter().any(|child| is_subject(world, child))
    })
}

fn is_subject(world: &World, entity: Entity) -> bool {
    world.get::<EntityUid>(entity).is_some()
}

/// What a row shows for a subject called `name`, with the id `uid`.
/// A subject with no id is no subject, and shows a question mark.
fn caption(name: Option<&Name>, uid: Option<EntityUid>) -> Caption {
    match uid {
        Some(uid) => Caption::entity(name, uid),
        None => Caption {
            name: None,
            head: "?".to_string(),
        },
    }
}
