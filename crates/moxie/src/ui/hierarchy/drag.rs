//! Moving a subject by dragging its row.
//!
//! Reordering and reparenting are the same edit: every subject sits
//! in some parent's [`Children`], so both are a matter of which list
//! a row lands in and where. One row is the whole drop target: its
//! middle means inside it, its top and bottom edges beside it. What
//! commits to a hair-thin line is a band a quarter of the row tall.

use bevy::picking::events::{
    Drag, DragDrop, DragEnd, DragLeave, DragOver, DragStart, Pointer,
};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{UiGlobalTransform, UiScale};
use bevy_fynix::{Bevy, Cx, Theme, View};
use bevy_motiongfx::scene::id::EntityUid;
use moxie_ui::cursor::PointerEventExt as _;
use moxie_ui::drag::{follow, ghost};
use moxie_ui::layout::logical_rect;
use moxie_ui::theme::EditorTheme;

use super::Collapsed;
use crate::SceneRoot;
use crate::subject::Caption;

/// How much of a row's height, at each end, aims beside it rather
/// than into it. The middle half is the drop-inside band.
const EDGE: f32 = 0.25;

/// What [`logical_rect`] reads off a node to place it in pointer
/// space.
type NodeRect = (&'static ComputedNode, &'static UiGlobalTransform);

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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum At {
    Before,
    Into,
    After,
}

/// A view whose root node is a row that can be picked up, and one a
/// drop can land on: before it, into it, or after it, by where in its
/// height it is aimed. See [`rows`].
pub(super) struct Rows<V> {
    inner: V,
    subject: Entity,
}

/// Makes `inner` the row of `subject`.
pub(super) fn rows<V>(inner: V, subject: Entity) -> Rows<V> {
    Rows { inner, subject }
}

impl<V: View<Bevy, EditorTheme>> View<Bevy, EditorTheme> for Rows<V> {
    fn build(self, cx: &mut Cx<'_, Bevy, EditorTheme>) -> Entity {
        let Self { inner, subject } = self;
        let row = inner.build(cx);

        cx.world
            .entity_mut(row)
            .observe(
                move |over: On<Pointer<DragOver>>,
                      scale: Res<UiScale>,
                      rects: Query<NodeRect>,
                      kids: Query<&Children>,
                      is_subject: Query<(), With<EntityUid>>,
                      is_collapsed: Query<(), With<Collapsed>>,
                      mut dragging: ResMut<Dragging>| {
                    if over.button != PointerButton::Primary {
                        return;
                    }
                    if dragging.subject.is_none() {
                        return;
                    }
                    let Ok((computed, transform)) = rects.get(row)
                    else {
                        return;
                    };
                    let rect = logical_rect(computed, transform);
                    let cursor = over.logical(&scale);
                    let frac =
                        (cursor.y - rect.min.y) / rect.height();

                    let at = if frac < EDGE {
                        At::Before
                    } else if frac > 1.0 - EDGE {
                        At::After
                    } else {
                        At::Into
                    };

                    // Below an open branch, the space is its
                    // children's: the drop lands as the first of
                    // them, which is the same slot, and the same
                    // line, as before that child.
                    let target = match at {
                        At::After if !is_collapsed.contains(subject) => {
                            kids.get(subject)
                                .ok()
                                .and_then(|kids| {
                                    kids.iter().find(|&kid| {
                                        is_subject.contains(kid)
                                    })
                                })
                                .map_or((subject, At::After), |first| {
                                    (first, At::Before)
                                })
                        }
                        _ => (subject, at),
                    };
                    dragging.target = Some(target);
                },
            )
            .observe(
                move |_: On<Pointer<DragLeave>>,
                      mut dragging: ResMut<Dragging>| {
                    // Aimed at the row itself, or at the first child
                    // that stands in for its lower edge.
                    if matches!(
                        dragging.target,
                        Some((aimed, _)) if aimed == subject
                    ) {
                        dragging.target = None;
                    }
                },
            )
            .observe(commit_drop)
            .observe(
                move |start: On<Pointer<DragStart>>,
                      names: Query<&Name>,
                      uids: Query<&EntityUid>,
                      theme: Res<Theme<EditorTheme>>,
                      scale: Res<UiScale>,
                      mut dragging: ResMut<Dragging>,
                      mut commands: Commands| {
                    if start.button != PointerButton::Primary {
                        return;
                    }

                    let Ok(&uid) = uids.get(subject) else {
                        return;
                    };
                    let name =
                        Caption::entity(names.get(subject).ok(), uid)
                            .text()
                            .to_string();
                    let at = start.logical(&scale);

                    dragging.subject = Some(subject);
                    dragging.ghost = Some(
                        commands
                            .spawn(ghost(at, name, &theme.0))
                            .id(),
                    );
                },
            )
            .observe(
                move |drag: On<Pointer<Drag>>,
                      scale: Res<UiScale>,
                      dragging: Res<Dragging>,
                      mut nodes: Query<&mut Node>| {
                    let Some(ghost) = dragging.ghost else {
                        return;
                    };
                    let Ok(mut node) = nodes.get_mut(ghost) else {
                        return;
                    };
                    follow(&mut node, drag.logical(&scale));
                },
            )
            .observe(
                move |_: On<Pointer<DragEnd>>,
                      mut dragging: ResMut<Dragging>,
                      mut commands: Commands| {
                    if let Some(ghost) = dragging.ghost.take() {
                        commands.entity(ghost).despawn();
                    }
                    dragging.subject = None;
                    dragging.target = None;
                },
            );
        row
    }
}

/// A view whose root node is the catch-all below the list: a drop
/// anywhere on it lands the dragged subject at the top level, right
/// after the last one, clear of any open branch. See [`below`].
pub(super) struct Below<V>(V);

/// Makes `inner` the drop zone below the list.
pub(super) fn below<V>(inner: V) -> Below<V> {
    Below(inner)
}

/// The last top-level subject, which a drop below the list lands
/// after. `None` with nothing to drag.
fn last_root(
    roots: &Query<&Children, With<SceneRoot>>,
    is_subject: &Query<(), With<EntityUid>>,
) -> Option<Entity> {
    roots
        .iter()
        .next()?
        .iter()
        .rev()
        .find(|&child| is_subject.contains(child))
}

impl<V: View<Bevy, EditorTheme>> View<Bevy, EditorTheme>
    for Below<V>
{
    fn build(self, cx: &mut Cx<'_, Bevy, EditorTheme>) -> Entity {
        let zone = self.0.build(cx);

        cx.world
            .entity_mut(zone)
            .observe(
                |over: On<Pointer<DragOver>>,
                 roots: Query<&Children, With<SceneRoot>>,
                 is_subject: Query<(), With<EntityUid>>,
                 mut dragging: ResMut<Dragging>| {
                    if over.button != PointerButton::Primary
                        || dragging.subject.is_none()
                    {
                        return;
                    }
                    dragging.target = last_root(&roots, &is_subject)
                        .map(|row| (row, At::After));
                },
            )
            .observe(
                |_: On<Pointer<DragLeave>>,
                 roots: Query<&Children, With<SceneRoot>>,
                 is_subject: Query<(), With<EntityUid>>,
                 mut dragging: ResMut<Dragging>| {
                    let last = last_root(&roots, &is_subject);
                    if matches!(
                        dragging.target,
                        Some((row, At::After)) if Some(row) == last
                    ) {
                        dragging.target = None;
                    }
                },
            )
            .observe(commit_drop);
        zone
    }
}

/// Queues the pending drop on a primary-button release, shared by
/// every kind of drop target. Read: the drop lands before [`DragEnd`]
/// clears the drag.
fn commit_drop(
    drop: On<Pointer<DragDrop>>,
    dragging: Res<Dragging>,
    mut commands: Commands,
) {
    if drop.button != PointerButton::Primary {
        return;
    }
    let (Some(dragged), Some((row, at))) =
        (dragging.subject, dragging.target)
    else {
        return;
    };
    commands.queue(move |world: &mut World| {
        apply(world, dragged, row, at);
    });
}

/// Moves `dragged` to where `at` puts it relative to `row`.
///
/// One [`insert_child`](EntityWorldMut::insert_child) does either
/// job: handed a child the parent already holds it moves it within
/// the list, and handed one it does not it takes it from wherever it
/// was.
fn apply(world: &mut World, dragged: Entity, row: Entity, at: At) {
    let Some((parent, index)) = destination(world, dragged, row, at)
    else {
        return;
    };

    // Read before the move, since attaching is what changes them.
    let local = world
        .get::<GlobalTransform>(dragged)
        .zip(world.get::<GlobalTransform>(parent))
        .map(|(dragged, parent)| dragged.reparented_to(parent));

    world.entity_mut(parent).insert_child(index, dragged);

    if let Some(local) = local {
        world.entity_mut(dragged).insert(local);
    }
}

/// The parent to put `dragged` under and where in its children, or
/// `None` for a drop that would not hold: onto itself, or into its
/// own subtree, which is a parent of its own parent.
fn destination(
    world: &World,
    dragged: Entity,
    row: Entity,
    at: At,
) -> Option<(Entity, usize)> {
    if dragged == row || holds(world, dragged, row) {
        return None;
    }

    let (parent, index) = match at {
        // Past the end, for a place at the end of the list.
        At::Into => (row, usize::MAX),
        _ => {
            let parent = world.get::<ChildOf>(row)?.parent();
            let index = world
                .get::<Children>(parent)?
                .iter()
                .position(|child| child == row)?;

            match at {
                At::After => (parent, index + 1),
                _ => (parent, index),
            }
        }
    };

    Some((parent, settle(world, parent, dragged, index)))
}

/// `index` as the list will actually be when the entity is put back.
///
/// Moving within one parent takes the entity out before putting it
/// back, so every slot after it shifts left. A forward move
/// overshoots by one; asking for the end overshoots the shortened
/// list entirely and panics instead of clamping.
fn settle(
    world: &World,
    parent: Entity,
    dragged: Entity,
    index: usize,
) -> usize {
    let Some(children) = world.get::<Children>(parent) else {
        return 0;
    };
    // Arriving from another parent is added first and only then
    // placed, so the list it lands in is never short.
    let Some(current) =
        children.iter().position(|child| child == dragged)
    else {
        return index;
    };

    let shifted = if index > current { index - 1 } else { index };

    shifted.min(children.len() - 1)
}

/// Whether `entity` is somewhere above `row`.
fn holds(world: &World, entity: Entity, row: Entity) -> bool {
    let mut above = world.get::<ChildOf>(row).map(ChildOf::parent);

    while let Some(parent) = above {
        if parent == entity {
            return true;
        }
        above = world.get::<ChildOf>(parent).map(ChildOf::parent);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parent with `count` children, in order.
    fn family(
        world: &mut World,
        count: usize,
    ) -> (Entity, Vec<Entity>) {
        let parent = world.spawn_empty().id();
        let kids = (0..count)
            .map(|_| world.spawn(ChildOf(parent)).id())
            .collect();
        (parent, kids)
    }

    fn kids_of(world: &World, parent: Entity) -> Vec<Entity> {
        world
            .get::<Children>(parent)
            .map(|kids| kids.iter().collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_forward_move_lands_after_its_target() {
        let mut world = World::new();
        let (parent, k) = family(&mut world, 3);

        apply(&mut world, k[0], k[1], At::After);

        assert_eq!(kids_of(&world, parent), [k[1], k[0], k[2]]);
    }

    #[test]
    fn a_backward_move_lands_before_its_target() {
        let mut world = World::new();
        let (parent, k) = family(&mut world, 3);

        apply(&mut world, k[2], k[0], At::Before);

        assert_eq!(kids_of(&world, parent), [k[2], k[0], k[1]]);
    }

    #[test]
    fn a_move_to_the_end_does_not_overshoot() {
        let mut world = World::new();
        let (parent, k) = family(&mut world, 3);

        apply(&mut world, k[0], k[2], At::After);

        assert_eq!(kids_of(&world, parent), [k[1], k[2], k[0]]);
    }

    #[test]
    fn dropping_into_a_row_makes_it_the_last_child() {
        let mut world = World::new();
        let (parent, k) = family(&mut world, 3);
        let grand = world.spawn(ChildOf(k[1])).id();

        apply(&mut world, k[0], k[1], At::Into);

        assert_eq!(kids_of(&world, parent), [k[1], k[2]]);
        assert_eq!(kids_of(&world, k[1]), [grand, k[0]]);
    }

    #[test]
    fn a_drop_onto_itself_or_its_own_subtree_is_refused() {
        let mut world = World::new();
        let (parent, k) = family(&mut world, 2);
        let below = world.spawn(ChildOf(k[0])).id();

        apply(&mut world, k[0], k[0], At::Into);
        apply(&mut world, k[0], below, At::After);

        assert_eq!(kids_of(&world, parent), [k[0], k[1]]);
        assert_eq!(kids_of(&world, k[0]), [below]);
    }

    #[test]
    fn a_reparented_row_keeps_its_place_in_the_world() {
        let mut world = World::new();
        let (parent, k) = family(&mut world, 2);
        world.entity_mut(parent).insert((
            Transform::from_xyz(10.0, 0.0, 0.0),
            GlobalTransform::from(Transform::from_xyz(
                10.0, 0.0, 0.0,
            )),
        ));
        world.entity_mut(k[0]).insert((
            Transform::default(),
            GlobalTransform::from(Transform::from_xyz(
                10.0, 0.0, 0.0,
            )),
        ));
        world.entity_mut(k[1]).insert((
            Transform::default(),
            GlobalTransform::from(Transform::from_xyz(
                30.0, 0.0, 0.0,
            )),
        ));

        apply(&mut world, k[0], k[1], At::Into);

        let local = world.get::<Transform>(k[0]).expect("placed");
        assert_eq!(local.translation, Vec3::new(-20.0, 0.0, 0.0));
    }
}
