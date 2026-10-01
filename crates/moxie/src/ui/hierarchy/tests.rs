//! The hierarchy panel, in the whole editor.

use bevy::camera::NormalizedRenderTarget;
use bevy::picking::backend::HitData;
use bevy::picking::events::{
    DragDrop, DragEnd, DragOver, DragStart, Pointer, Press,
};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::ui_widgets::Button as ButtonBehavior;
use bevy_motiongfx::scene::id::EntityUid;
use moxie_ui::layout::logical_rect;

use super::{At, Collapsed, Dragging};
use crate::tests::harness::{Editor, SETTLE};
use crate::{SceneRoot, SelectedEntity};

/// A subject called `name` under `parent`, or at the top level.
fn subject(
    editor: &mut Editor,
    name: &str,
    parent: Option<Entity>,
) -> Entity {
    let world = editor.world();
    let parent = parent.unwrap_or_else(|| {
        world
            .query_filtered::<Entity, With<SceneRoot>>()
            .single(world)
            .expect("the scene root")
    });
    let entity = world
        .spawn((EntityUid::new(), Name::new(name.to_string())))
        .insert(ChildOf(parent))
        .id();
    editor.step(SETTLE);
    entity
}

fn children(editor: &mut Editor, parent: Entity) -> Vec<Entity> {
    editor
        .world()
        .get::<Children>(parent)
        .map(|kids| kids.iter().collect())
        .unwrap_or_default()
}

/// The row button showing `name`.
fn row_of(editor: &mut Editor, name: &str) -> Entity {
    let mut at = editor.text(name);
    let world = editor.world();
    while world.get::<ButtonBehavior>(at).is_none() {
        at = world.get::<ChildOf>(at).expect("in a row").parent();
    }
    at
}

/// Sends `event` to `on`, and runs a frame.
fn fire<E>(editor: &mut Editor, on: Entity, event: E)
where
    E: core::fmt::Debug + Clone + Reflect,
    Pointer<E>: bevy::ecs::event::Event,
    for<'t> <Pointer<E> as bevy::ecs::event::Event>::Trigger<'t>:
        Default,
{
    send(editor, on, event);
    editor.step(1);
}

/// Sends `event` to `on`, with the pointer at the middle of it.
fn send<E>(editor: &mut Editor, on: Entity, event: E)
where
    E: core::fmt::Debug + Clone + Reflect,
    Pointer<E>: bevy::ecs::event::Event,
    for<'t> <Pointer<E> as bevy::ecs::event::Event>::Trigger<'t>:
        Default,
{
    let world = editor.world();
    let position = world
        .get::<ComputedNode>(on)
        .zip(world.get::<UiGlobalTransform>(on))
        .map_or(Vec2::ZERO, |(computed, transform)| {
            logical_rect(computed, transform).center()
        });
    let location = Location {
        target: NormalizedRenderTarget::None {
            width: 1,
            height: 1,
        },
        position,
    };
    editor.world().trigger(Pointer::new(
        PointerId::Mouse,
        location,
        event,
        on,
    ));
}

fn hit() -> HitData {
    HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
}

fn end() -> DragEnd {
    DragEnd {
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    }
}

/// Starts dragging `from`'s row and aims it at `over`'s.
fn aim(editor: &mut Editor, from: Entity, over: Entity) {
    let button = PointerButton::Primary;
    fire(editor, from, DragStart { button, hit: hit() });
    fire(
        editor,
        over,
        DragOver {
            button,
            dragged: from,
            hit: hit(),
        },
    );
}

/// Drags `from`'s row onto `over`'s and lets go there.
fn drop_on(editor: &mut Editor, from: &str, over: &str) {
    let from = row_of(editor, from);
    let over = row_of(editor, over);
    aim(editor, from, over);
    // Back to back, as the pointer sends them: the row is rebuilt only
    // on the next frame.
    send(
        editor,
        over,
        DragDrop {
            button: PointerButton::Primary,
            dropped: from,
            hit: hit(),
        },
    );
    send(editor, from, end());
    editor.step(SETTLE);
}

#[test]
fn the_add_menu_offers_every_preset() {
    let mut editor = Editor::new();
    let add = editor.named("Add");
    editor.press_entity(add);

    for name in [
        "Empty",
        "Cube",
        "Sphere",
        "Plane",
        "Cylinder",
        "Cone",
        "Torus",
        "Monkey",
        "Point Light",
        "Directional Light",
    ] {
        editor.text(name);
    }
}

#[test]
fn the_add_menu_makes_an_empty_subject_selected() {
    let mut editor = Editor::new();
    let add = editor.named("Add");
    editor.press_entity(add);
    editor.press("Empty");

    let world = editor.world();
    let selected = world
        .resource::<SelectedEntity>()
        .0
        .expect("it is selected");
    assert!(world.get::<EntityUid>(selected).is_some());
    assert!(world.get::<Mesh3d>(selected).is_none());
}

#[test]
fn pressing_a_row_selects_its_subject() {
    let mut editor = Editor::new();
    let alpha = subject(&mut editor, "Alpha", None);
    let beta = subject(&mut editor, "Beta", None);

    editor.press("Alpha");
    let selected = editor.world().resource::<SelectedEntity>().0;
    assert_eq!(selected, Some(alpha));
    editor.press("Beta");
    let selected = editor.world().resource::<SelectedEntity>().0;
    assert_eq!(selected, Some(beta));
}

#[test]
fn a_row_shows_the_subjects_name_as_it_changes() {
    let mut editor = Editor::new();
    let alpha = subject(&mut editor, "Alpha", None);

    editor.world().entity_mut(alpha).insert(Name::new("Omega"));
    editor.step(SETTLE);

    editor.text("Omega");
    assert!(editor.texts("Alpha").is_empty());
}

#[test]
fn a_child_row_folds_away_with_its_chevron() {
    let mut editor = Editor::new();
    let parent = subject(&mut editor, "Parent", None);
    subject(&mut editor, "Child", Some(parent));
    editor.text("Child");

    // The fold's head holds a chevron button, then the row's slot.
    let header = row_of(&mut editor, "Parent");
    let world = editor.world();
    let slot =
        world.get::<ChildOf>(header).expect("in a slot").parent();
    let head =
        world.get::<ChildOf>(slot).expect("in a head").parent();
    let toggle = world
        .get::<Children>(head)
        .expect("a head")
        .iter()
        .next()
        .expect("a chevron");
    assert_ne!(toggle, slot);

    editor.press_entity(toggle);
    assert!(editor.texts("Child").is_empty(), "folded away");
    assert!(editor.world().get::<Collapsed>(parent).is_some());

    editor.press_entity(toggle);
    editor.text("Child");
    assert!(editor.world().get::<Collapsed>(parent).is_none());
}

#[test]
fn a_collapsed_branch_stays_collapsed_when_it_moves() {
    let mut editor = Editor::new();
    let a = subject(&mut editor, "A", None);
    subject(&mut editor, "Under A", Some(a));
    let b = subject(&mut editor, "B", None);
    editor.world().entity_mut(a).insert(Collapsed);
    editor.step(SETTLE);

    // Dropped into B, A's row is built again.
    drop_on(&mut editor, "A", "B");

    assert_eq!(children(&mut editor, b), [a]);
    assert!(editor.world().get::<Collapsed>(a).is_some());
    assert!(editor.texts("Under A").is_empty());
}

#[test]
fn dropping_a_row_into_another_reparents_it() {
    let mut editor = Editor::new();
    let alpha = subject(&mut editor, "Alpha", None);
    let beta = subject(&mut editor, "Beta", None);

    drop_on(&mut editor, "Alpha", "Beta");

    assert_eq!(children(&mut editor, beta), [alpha]);
    // Its row is under Beta's now, still showing.
    editor.text("Alpha");
    let dragging = editor.world().resource::<Dragging>();
    assert!(!dragging.shows(beta, At::Into), "the drag is over");
}

#[test]
fn a_drag_aimed_at_a_row_lights_it_until_it_ends() {
    let mut editor = Editor::new();
    let alpha = subject(&mut editor, "Alpha", None);
    let beta = subject(&mut editor, "Beta", None);
    let alpha_row = row_of(&mut editor, "Alpha");
    let beta_row = row_of(&mut editor, "Beta");

    aim(&mut editor, alpha_row, beta_row);
    assert!(
        editor.world().resource::<Dragging>().shows(beta, At::Into)
    );

    fire(&mut editor, alpha_row, end());
    assert!(
        !editor.world().resource::<Dragging>().shows(beta, At::Into)
    );
    assert!(editor.world().get::<ChildOf>(alpha).is_some());
}

#[test]
fn letting_go_outside_any_row_changes_nothing() {
    let mut editor = Editor::new();
    let alpha = subject(&mut editor, "Alpha", None);
    let beta = subject(&mut editor, "Beta", None);
    let alpha_row = row_of(&mut editor, "Alpha");
    let beta_row = row_of(&mut editor, "Beta");

    aim(&mut editor, alpha_row, beta_row);
    fire(&mut editor, alpha_row, end());
    editor.step(SETTLE);

    assert!(children(&mut editor, beta).is_empty());
    assert!(editor.world().get::<ChildOf>(alpha).is_some());
}

#[test]
fn the_context_menu_deletes_a_subject_and_its_selection() {
    let mut editor = Editor::new();
    let parent = subject(&mut editor, "Parent", None);
    let child = subject(&mut editor, "Child", Some(parent));
    subject(&mut editor, "Keep", None);
    editor.world().insert_resource(SelectedEntity(Some(child)));

    let row = row_of(&mut editor, "Parent");
    fire(
        &mut editor,
        row,
        Press {
            button: PointerButton::Secondary,
            hit: hit(),
            count: 1,
        },
    );
    editor.step(SETTLE);
    editor.press("Delete");

    let world = editor.world();
    assert!(world.get_entity(parent).is_err());
    assert!(world.get_entity(child).is_err());
    assert_eq!(world.resource::<SelectedEntity>().0, None);
    editor.text("Keep");
    assert!(editor.texts("Parent").is_empty());
}
