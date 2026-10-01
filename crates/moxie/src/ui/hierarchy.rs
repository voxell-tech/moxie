//! Scene hierarchy browser: an indented list of the scene's subjects.
//!
//! What counts as one is an [`EntityUid`], the id a scene refers to
//! its subjects by. The panel lists exactly what the animation can
//! address, nothing the editor spawned for itself.

// STUB: ported in wave 3 step 2. Only the panel's view is a
// placeholder; what it will call is kept below.

use bevy::ecs::query::QueryState;
use bevy::ecs::reflect::ReflectComponent;
use bevy::prelude::*;
use bevy_fynix::{AnyView, Bevy};
use bevy_motiongfx::scene::id::EntityUid;
use moxie_ui::inspector::ReflectEssential;
use moxie_ui::theme::EditorTheme;

use crate::subject::Caption;
use crate::{SceneRoot, SelectedEntity, presets};

/// The hierarchy panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    // STUB: ported in wave 3 step 2
    super::stub_panel("Hierarchy")
}

/// The subject being dragged, where a drop would land it, and what is
/// following the cursor meanwhile. Empty whenever nothing is being
/// dragged.
#[derive(Resource, Default)]
pub(crate) struct Dragging {
    /// The subject: a pointer event names whichever node it hit,
    /// which may be a row's label, and neither is the thing being
    /// moved.
    subject: Option<Entity>,
    target: Option<(Entity, At)>,
    ghost: Option<Entity>,
}

impl Dragging {
    /// Whether a drop would land at `at` relative to `row`.
    pub(crate) fn shows(&self, row: Entity, at: At) -> bool {
        self.target == Some((row, at))
    }
}

/// Where a drop would put what is being dragged, relative to the row
/// it is aimed at.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    Before,
    Into,
    After,
}

/// Spawns a subject at the top level, and selects it so the inspector
/// is already pointed at what was just made.
///
/// Nothing of the animation changes: a [`Stage`](motiongfx_scene::scene::Stage)
/// seeds the fields an action drives, and a subject with no action on
/// it keeps whatever it was spawned holding.
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

/// `None` once `entity` is no subject.
fn caption_of(world: &World, entity: Entity) -> Option<Caption> {
    let uid = *world.get::<EntityUid>(entity)?;
    Some(Caption::entity(world.get::<Name>(entity), uid))
}

fn name_of(world: &World, entity: Entity) -> String {
    caption_of(world, entity).map_or_else(
        || "?".to_string(),
        |caption| caption.text().to_string(),
    )
}

/// Whether [`name_of`] is standing in for a name `entity` doesn't
/// have.
fn is_unnamed(world: &World, entity: Entity) -> bool {
    caption_of(world, entity)
        .is_none_or(|caption| caption.name.is_none())
}
