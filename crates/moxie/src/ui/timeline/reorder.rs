//! Dragging a node's body elsewhere in the tree. Action leaves and
//! block headers share this, each just a node at a path. Where it
//! lands is [`landing`]'s.
//!
//! The tree is written only when the drag ends. Until then the
//! dragged box and its subtree are the preview, offset by how far the
//! cursor has moved, while the rest of the layout stays put. A slim
//! line or an outline marks where a release would land.

use bevy::picking::events::{DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform, UiScale};
use bevy_fynix::{AnyView, Bevy, OverrideCursor, Theme, View};
use bevy_motiongfx::scene::backend::Backend;
use motiongfx_scene::block::{Block, Node as SceneNode};
use moxie_ui::cursor::{Cursor, PointerEventExt as _};
use moxie_ui::drag::{Dragged, grab, ungrab};
use moxie_ui::layout::logical_rect;
use moxie_ui::theme::EditorTheme;

use super::hint::HintNode;
use super::landing::{self, Target, block_at_mut, under};
use super::retime::{BoxPath, GapPath};
use super::{
    BlockFoldState, RebuildTick, TrackViewport, block_layout, prune,
};
use crate::scene::{is_track, normalize};
use crate::{EditorScene, SelectedAction, TimelineView};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Dragging>()
        .add_systems(Update, cancel_on_escape)
        .add_systems(Update, preview.run_if(Dragging::active))
        .add_observer(on_drag_end);
}

/// The node being dragged, if any.
#[derive(Resource, Default)]
struct Dragging(Option<Gesture>);

impl Dragging {
    fn active(dragging: Res<Self>) -> bool {
        dragging.0.is_some()
    }
}

/// One drag in progress.
struct Gesture {
    path: Vec<usize>,
    cursor_start: Vec2,
    /// The cursor's offset inside the box, set on the first preview.
    hold: Option<Vec2>,
    target: Option<Target>,
}

/// `handle` as a node's own body: dragging it moves `path` elsewhere
/// in the tree.
pub(crate) fn body(
    handle: impl View<Bevy, EditorTheme> + 'static,
    path: Vec<usize>,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::new(move |cx| {
        let node = cx.build(handle);
        // Not `event_target()`: neither a label nor an icon ignores
        // the pointer, so a grab on the text or the chevron reports
        // that child instead of the box being wired here.
        cx.world.entity_mut(node).observe(
            move |start: On<Pointer<DragStart>>,
                  scale: Res<UiScale>,
                  q_viewport: Query<
                (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
                With<TrackViewport>,
            >,
                  q_boxes: Query<(Entity, &BoxPath)>,
                  q_children: Query<&Children>,
                  mut dragging: ResMut<Dragging>,
                  mut override_cursor: ResMut<OverrideCursor>,
                  mut commands: Commands| {
                if start.button != PointerButton::Primary
                    || is_track(&path)
                    || path.is_empty()
                {
                    // The root and the tracks stay where they are.
                    return;
                }
                let Ok((node, transform, scroll)) =
                    q_viewport.single()
                else {
                    return;
                };

                let cursor = start.logical(&scale);
                grab(&mut override_cursor);
                // The box, not `start.entity`: a block's handle is
                // its header button, inside the box. The rebuild that
                // ends the drag respawns everything, so the state
                // never needs clearing.
                if let Some((dragged, _)) = q_boxes
                    .iter()
                    .find(|(_, box_path)| box_path.0 == path)
                {
                    for node in core::iter::once(dragged)
                        .chain(q_children.iter_descendants(dragged))
                    {
                        commands.entity(node).insert(Dragged);
                    }
                }
                dragging.0 = Some(Gesture {
                    path: path.clone(),
                    cursor_start: to_content(
                        cursor, node, transform, scroll,
                    ),
                    hold: None,
                    target: None,
                });
            },
        );
        node
    })
}

/// Each frame of a drag: lays the tree out, offsets the dragged
/// subtree to the cursor, and marks where a release would land.
fn preview(
    theme: Res<Theme<EditorTheme>>,
    pointer: Cursor,
    hint: HintNode,
    editor_scene: Res<EditorScene>,
    folded: Res<BlockFoldState>,
    view: Res<TimelineView>,
    mut dragging: ResMut<Dragging>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    q_boxes: Query<(Entity, &BoxPath, Option<&ChildOf>)>,
    q_gaps: Query<(Entity, &GapPath), Without<BoxPath>>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    let Some(gesture) = dragging.0.as_mut() else {
        return;
    };
    let Some(cursor) = pointer.position() else {
        return;
    };
    let Ok((viewport_node, viewport_transform, scroll)) =
        q_viewport.single()
    else {
        return;
    };
    let viewport_rect =
        logical_rect(viewport_node, viewport_transform);
    let drag_z = theme.0.layer.drag;

    let content = Vec2::new(
        cursor.x - viewport_rect.min.x,
        cursor.y - viewport_rect.min.y + scroll.y,
    );
    let root = &editor_scene.scene().0.animation;
    let layout = block_layout::layout(
        root,
        *view,
        folded.paths(),
        theme.0.space,
    );

    // Detaches to the tracks' parent, which sits at the content's
    // origin. The rebuild that ends the drag puts it back.
    let drag_parent = q_boxes
        .iter()
        .find(|(_, box_path, _)| is_track(&box_path.0))
        .and_then(|(_, _, child_of)| child_of.map(ChildOf::parent));
    let Some(placed) = layout.iter().find(|p| p.path == gesture.path)
    else {
        return;
    };
    let hold = *gesture.hold.get_or_insert(
        gesture.cursor_start - Vec2::new(placed.x, placed.y),
    );
    let at = content - hold;

    for (entity, box_path, child_of) in &q_boxes {
        if box_path.0 != gesture.path {
            continue;
        }
        commands.entity(entity).insert(GlobalZIndex(drag_z));
        if let Some(drag_parent) = drag_parent
            && child_of.map(ChildOf::parent) != Some(drag_parent)
        {
            commands.entity(entity).insert(ChildOf(drag_parent));
        }
        if let Ok(mut node) = nodes.get_mut(entity) {
            node.left = px(at.x);
            node.top = px(at.y);
            node.width = px(placed.w);
            node.height = px(placed.h);
        }
    }

    for (entity, gap_path) in &q_gaps {
        if gap_path.0 != gesture.path {
            continue;
        }
        commands.entity(entity).insert(GlobalZIndex(drag_z));
        if let Some(drag_parent) = drag_parent {
            commands.entity(entity).insert(ChildOf(drag_parent));
        }
        if let Some(gap_x) = placed.gap_x
            && let Ok(mut node) = nodes.get_mut(entity)
        {
            node.left = px(at.x - (placed.x - gap_x));
            node.top = px(at.y);
            node.width = px(placed.x - gap_x);
        }
    }

    gesture.target =
        landing::resolve(content, &layout, root, Some(&gesture.path));
    landing::announce_hint(
        &mut commands,
        &hint,
        &theme.0,
        gesture.target.as_ref(),
        &layout,
        root,
    );
}

/// A cursor in logical screen space, mapped into the viewport's
/// content space, where the `Placed`s live.
pub(super) fn to_content(
    cursor: Vec2,
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    scroll: &ScrollPosition,
) -> Vec2 {
    let min = logical_rect(node, transform).min;
    Vec2::new(cursor.x - min.x, cursor.y - min.y + scroll.y)
}

/// Common tail of a committed drop and a cancel: drop the drag-wide
/// cursor and bump [`RebuildTick`], so the box list respawns and
/// every dragged box loses both its preview offset and its raised z.
fn end_drag(
    override_cursor: &mut OverrideCursor,
    commands: &mut Commands,
) {
    ungrab(override_cursor);
    commands.queue(RebuildTick::bump_in);
}

/// Ends the `body` drag in progress and commits the drop, unless it
/// settled where it started.
fn on_drag_end(
    _: On<Pointer<DragEnd>>,
    hint: HintNode,
    mut dragging: ResMut<Dragging>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    let Some(gesture) = dragging.0.take() else {
        return;
    };
    hint.hide(&mut commands);
    end_drag(&mut override_cursor, &mut commands);

    let Some(target) = gesture.target else {
        return;
    };
    if settles_where_it_started(&target, &gesture.path) {
        return;
    }
    commands.queue(move |world: &mut World| {
        commit(world, &gesture.path, &target);
    });
}

/// Drops whatever's being dragged without committing it.
fn cancel_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    hint: HintNode,
    mut dragging: ResMut<Dragging>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if dragging.0.take().is_none() {
        return;
    }
    hint.hide(&mut commands);
    end_drag(&mut override_cursor, &mut commands);
}

/// Whether `target` puts the node back exactly where it already is.
fn settles_where_it_started(target: &Target, from: &[usize]) -> bool {
    let Target::Insert { parent, index } = target else {
        return false;
    };
    let Some((from_index, from_parent)) = from.split_last() else {
        return true;
    };
    parent == from_parent
        && (*index == *from_index || *index == from_index + 1)
}

//
// Committing.
//

/// Removes the node at `path`, pruning any block left empty by it and
/// rebasing or clearing the selection the same way a drop does.
pub(crate) fn delete(world: &mut World, path: &[usize]) {
    let mut kept = world
        .get_resource::<SelectedAction>()
        .and_then(|selected| selected.0.clone())
        .and_then(|selected| after_removal(&selected, path));

    let Some(mut editor_scene) =
        world.get_resource_mut::<EditorScene>()
    else {
        return;
    };
    if take(&mut editor_scene.edit().animation, path).is_none() {
        return;
    }
    prune::in_tracks(&mut editor_scene.edit().animation, &mut kept);
    // Taking the last track leaves one empty track behind.
    normalize(&mut editor_scene.edit().animation);
    prune::stage(editor_scene.edit());

    if let Some(mut selected) =
        world.get_resource_mut::<SelectedAction>()
    {
        selected.0 = kept;
    }
    RebuildTick::bump_in(world);
}

/// Writes the drop's result back into the scene, following the
/// selection if it named the moved node (or one inside it).
fn commit(world: &mut World, from: &[usize], target: &Target) {
    // The selection's tail past `from`, if it points into the moved
    // node; re-based onto wherever the node lands.
    let selected_tail = world
        .get_resource::<SelectedAction>()
        .and_then(|selected| selected.0.clone())
        .filter(|path| under(path, from))
        .map(|path| path[from.len()..].to_vec());

    let Some(mut editor_scene) =
        world.get_resource_mut::<EditorScene>()
    else {
        return;
    };
    let Some(landed) =
        relocate(&mut editor_scene.edit().animation, from, target)
    else {
        return;
    };

    // After the move: pruning renumbers paths, and `target` was
    // resolved against the tree as it stood.
    let mut kept = selected_tail.map(|tail| {
        let mut path = landed;
        path.extend(tail);
        path
    });
    prune::in_tracks(&mut editor_scene.edit().animation, &mut kept);

    if let Some(mut selected) =
        world.get_resource_mut::<SelectedAction>()
        && selected.0.as_deref().is_some_and(|path| under(path, from))
    {
        selected.0 = kept;
    }
    RebuildTick::bump_in(world);
}

/// Moves `from` to `target`, resolved against the tree with `from`
/// still in it, returning where it landed.
fn relocate(
    root: &mut Block<Backend>,
    from: &[usize],
    target: &Target,
) -> Option<Vec<usize>> {
    if is_track(from) {
        return None;
    }
    let (from_index, from_parent) = from.split_last()?;
    let node = take(root, from)?;

    let target = match target {
        Target::Insert { parent, index } => {
            let parent = after_removal(parent, from)?;
            // Removing the node closed the gap it left, so an index
            // past it in the same block is now one too far.
            let index =
                if parent == from_parent && *index > *from_index {
                    index - 1
                } else {
                    *index
                };
            Target::Insert { parent, index }
        }
        Target::Merge {
            path,
            combinator,
            before,
        } => Target::Merge {
            path: after_removal(path, from)?,
            combinator: combinator.clone(),
            before: *before,
        },
    };
    landing::place(root, &target, node)
}

/// Pulls the node at `path` out of the tree.
fn take(
    root: &mut Block<Backend>,
    path: &[usize],
) -> Option<SceneNode<Backend>> {
    let (index, parent) = path.split_last()?;
    let block = block_at_mut(root, parent)?;
    (*index < block.children.len())
        .then(|| block.children.remove(*index))
}

/// Where `path` ends up once the node at `removed` is taken out -
/// `None` for a path inside that node, which goes with it.
fn after_removal(
    path: &[usize],
    removed: &[usize],
) -> Option<Vec<usize>> {
    let (index, parent) = removed.split_last()?;
    if path.len() <= parent.len()
        || path[..parent.len()] != parent[..]
    {
        return Some(path.to_vec());
    }

    let at = path[parent.len()];
    if at == *index {
        return None;
    }
    let mut out = path.to_vec();
    if at > *index {
        out[parent.len()] = at - 1;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use motiongfx_scene::block::Combinator;

    use super::*;

    fn delayed(secs: u64) -> SceneNode<Backend> {
        SceneNode::Draft {
            delay: Some(Duration::from_secs(secs)),
            duration: Duration::from_secs(1),
            name: None,
        }
    }

    #[test]
    fn moving_later_in_its_own_block_lands_where_aimed() {
        let mut root = Block {
            combinator: Combinator::All,
            children: vec![track(vec![
                delayed(1),
                delayed(2),
                delayed(3),
            ])],
            name: None,
        };

        // Between the second and the third, counted with the first
        // still in place.
        let landed = relocate(
            &mut root,
            &[0, 0],
            &Target::Insert {
                parent: vec![0],
                index: 2,
            },
        );

        assert_eq!(landed, Some(vec![0, 1]));
        let SceneNode::Block { block, .. } = &root.children[0] else {
            panic!("a track");
        };
        let delays =
            block.children.iter().map(delay).collect::<Vec<_>>();
        assert_eq!(
            delays,
            [2, 1, 3].map(|secs| Some(Duration::from_secs(secs)))
        );
    }

    fn delay(node: &SceneNode<Backend>) -> Option<Duration> {
        match node {
            SceneNode::Block { delay, .. }
            | SceneNode::Action { delay, .. }
            | SceneNode::Draft { delay, .. } => *delay,
        }
    }

    #[test]
    fn merging_leaves_each_delay_on_its_own_node() {
        let mut root = Block {
            combinator: Combinator::All,
            children: vec![track(vec![delayed(2), delayed(3)])],
            name: None,
        };

        let landed = relocate(
            &mut root,
            &[0, 1],
            &Target::Merge {
                path: vec![0, 0],
                combinator: Combinator::Chain,
                before: false,
            },
        );

        assert_eq!(landed, Some(vec![0, 0, 1]));
        let SceneNode::Block { block: track, .. } = &root.children[0]
        else {
            panic!("a track");
        };
        let SceneNode::Block {
            delay: wrapper,
            block,
        } = &track.children[0]
        else {
            panic!("the pair should be wrapped in a block");
        };
        assert_eq!(*wrapper, None);
        assert_eq!(
            delay(&block.children[0]),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            delay(&block.children[1]),
            Some(Duration::from_secs(3))
        );
    }

    fn track(
        children: Vec<SceneNode<Backend>>,
    ) -> SceneNode<Backend> {
        SceneNode::block(Block::chain(children))
    }

    /// A world whose scene holds `tracks`.
    fn world_of(tracks: Vec<SceneNode<Backend>>) -> World {
        let mut scene = EditorScene::default();
        scene.edit().animation.children = tracks;
        let mut world = World::new();
        world.insert_resource(scene);
        world
    }

    fn tracks_of(world: &World) -> &[SceneNode<Backend>] {
        &world.resource::<EditorScene>().scene().0.animation.children
    }

    #[test]
    fn deleting_the_last_track_leaves_an_empty_one() {
        let mut world = world_of(vec![track(vec![delayed(1)])]);
        delete(&mut world, &[0]);

        assert_eq!(tracks_of(&world), [track(Vec::new())]);
    }

    #[test]
    fn deleting_one_of_several_tracks_keeps_the_rest() {
        let mut world = world_of(vec![
            track(vec![delayed(1)]),
            track(vec![delayed(2)]),
        ]);
        delete(&mut world, &[0]);

        assert_eq!(tracks_of(&world), [track(vec![delayed(2)])]);
    }

    #[test]
    fn emptying_a_track_keeps_it() {
        let mut world = world_of(vec![
            track(vec![SceneNode::block(Block::chain(vec![
                delayed(1),
            ]))]),
            track(vec![delayed(2)]),
        ]);
        delete(&mut world, &[0, 0, 0]);

        assert_eq!(
            tracks_of(&world),
            [track(Vec::new()), track(vec![delayed(2)])]
        );
    }

    #[test]
    fn a_track_cannot_be_moved() {
        let mut root = Block {
            combinator: Combinator::All,
            children: vec![track(vec![]), track(vec![])],
            name: None,
        };
        let before = root.clone();
        let landed = relocate(
            &mut root,
            &[0],
            &Target::Insert {
                parent: vec![],
                index: 2,
            },
        );

        assert_eq!(landed, None);
        assert_eq!(root, before);
    }

    #[test]
    fn a_node_moves_between_tracks() {
        let mut root = Block {
            combinator: Combinator::All,
            children: vec![track(vec![delayed(1)]), track(vec![])],
            name: None,
        };
        let landed = relocate(
            &mut root,
            &[0, 0],
            &Target::Insert {
                parent: vec![1],
                index: 0,
            },
        );

        assert_eq!(landed, Some(vec![1, 0]));
        assert_eq!(
            root.children,
            [track(vec![]), track(vec![delayed(1)])]
        );
    }
}
