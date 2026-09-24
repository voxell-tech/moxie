//! Dragging a node's body elsewhere in the tree: between two of a
//! block's children, or onto another node to wrap the pair in a new
//! block. Action leaves and block headers share this, each just a node
//! at a path.
//!
//! The tree is written only when the drag ends. Until then the dragged
//! box and its subtree are the preview, offset by how far the cursor
//! has moved, while the rest of the layout stays put. A slim line or
//! an outline marks where a release would land.

use bevy::feathers::cursor::OverrideCursor;
use bevy::picking::events::{DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform, UiScale};
use bevy_fynix::WorldEntityMut;
use bevy_motiongfx::scene::backend::Backend;
use fynix::element::Element;
use fynix::ui::ElementMut;
use motiongfx_scene::block::{Block, Combinator, Node as SceneNode};
use moxie_ui::cursor::{Cursor, PointerEventExt as _};
use moxie_ui::drag::{Dragged, grab, ungrab};
use moxie_ui::layout::logical_rect;
use moxie_ui::reactive::{BevyFynix, FynixHost, FynixSet};
use moxie_ui::theme::EditorTheme;

use super::block_layout::{self, HEADER_HEIGHT, Placed};
use super::hint::{HideLanding, ShowLanding};
use super::prune;
use super::retime::{BoxPath, GapPath};
use super::{BlockFoldState, RebuildTick, TrackViewport};
use crate::{EditorScene, SelectedAction, TimelineView};

/// How close to a node's own edge a drop stops being about that node
/// and starts being about the block around it.
const EDGE_MARGIN_PX: f32 = 8.0;
/// How much of a node's core, at either end, chains rather than
/// overlaps.
const CHAIN_BAND: f32 = 0.25;

/// Registers the drag state, the preview, and what ends a drag.
pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Dragging>()
        .add_systems(Update, cancel_on_escape)
        .add_systems(
            Update,
            preview.run_if(Dragging::active).after(FynixSet),
        )
        .add_observer(on_drag_end);
}

/// The node being dragged, if any.
#[derive(Resource, Default)]
struct Dragging(Option<Gesture>);

impl Dragging {
    /// Whether a node is being dragged, as a run condition.
    fn active(dragging: Res<Self>) -> bool {
        dragging.0.is_some()
    }
}

/// Where a dragged node lands when released.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    /// Among `parent`'s children, at `index`.
    Insert { parent: Vec<usize>, index: usize },
    /// Onto `path`, wrapping the two of them in a new block under
    /// `combinator`. `before` puts the dragged node first.
    Merge {
        path: Vec<usize>,
        combinator: Combinator,
        before: bool,
    },
}

/// One drag in progress.
struct Gesture {
    path: Vec<usize>,
    cursor_start: Vec2,
    /// Where the cursor sits inside the box, set on the first preview.
    hold: Option<Vec2>,
    target: Option<Target>,
}

/// The axis a block orders its children along.
#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn of(self, point: Vec2) -> f32 {
        match self {
            Axis::X => point.x,
            Axis::Y => point.y,
        }
    }
}

/// Makes `handle` a node's own body: dragging it moves `path`
/// elsewhere in the tree.
pub(crate) fn body<'r, 'u, 'a, E: Element<FynixHost>>(
    handle: &'r mut ElementMut<'u, 'a, FynixHost, E>,
    path: Vec<usize>,
) -> &'r mut ElementMut<'u, 'a, FynixHost, E> {
    // Not `event_target()`: neither `Label` nor `Icon` ignores the
    // pointer, so a grab on the text or the chevron reports that
    // child instead of the box being wired here.
    handle.observe(
        move |start: On<Pointer<DragStart>>,
              scale: Res<UiScale>,
              q_viewport: Query<
            (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
            With<TrackViewport>,
        >,
              q_boxes: Query<(Entity, &BoxPath)>,
              q_children: Query<&Children>,
              mut kernel: ResMut<BevyFynix>,
              mut dragging: ResMut<Dragging>,
              mut override_cursor: ResMut<OverrideCursor>| {
            if start.button != PointerButton::Primary
                || path.is_empty()
            {
                // The root has nowhere to land.
                return;
            }
            let Ok((node, transform, scroll)) = q_viewport.single()
            else {
                return;
            };

            let cursor = start.logical(&scale);
            grab(&mut override_cursor);
            // The box, not `start.entity`: a block's handle is its
            // header button, inside the box. The rebuild that ends the
            // drag respawns everything, so the tag never needs clearing.
            if let Some((dragged, _)) = q_boxes
                .iter()
                .find(|(_, box_path)| box_path.0 == path)
            {
                for node in core::iter::once(dragged)
                    .chain(q_children.iter_descendants(dragged))
                {
                    kernel.set_tag(node, Dragged);
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
    )
}

/// Each frame of a drag: lays the tree out, offsets the dragged
/// subtree to the cursor, and marks where a release would land.
fn preview(
    kernel: Res<BevyFynix>,
    pointer: Cursor,
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
    let drag_z = kernel.theme().layer.drag;

    let content = Vec2::new(
        cursor.x - viewport_rect.min.x,
        cursor.y - viewport_rect.min.y + scroll.y,
    );
    let root = &editor_scene.scene().0.animation;
    let layout = block_layout::layout(root, *view, folded.paths());

    // Detaches to the root's parent.
    // The rebuild that ends the drag puts it back.
    let drag_parent = q_boxes
        .iter()
        .find(|(_, box_path, _)| box_path.0.is_empty())
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

    gesture.target = resolve(content, &layout, root, &gesture.path);
    announce_landing(
        &mut commands,
        kernel.theme(),
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
/// cursor and bump [`RebuildTick`], so the box list respawns and every
/// dragged box loses both its preview offset and its raised z.
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
    mut dragging: ResMut<Dragging>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    let Some(gesture) = dragging.0.take() else {
        return;
    };
    commands.trigger(HideLanding);
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
    commands.trigger(HideLanding);
    end_drag(&mut override_cursor, &mut commands);
}

//
// Where a release lands.
//

/// Where releasing at `cursor` would put the node at `dragged`.
pub(super) fn resolve(
    cursor: Vec2,
    layout: &[Placed],
    root: &Block<Backend>,
    dragged: &[usize],
) -> Option<Target> {
    let parent = innermost_block(cursor, layout, dragged)?;
    let axis = axis_of(root, &parent)?;
    let children = layout
        .iter()
        .filter(|placed| is_child_of(&placed.path, &parent))
        .collect::<Vec<_>>();

    let combinator = block_at(root, &parent)?.combinator.clone();

    for child in &children {
        let bounds = rect(child);
        if !bounds.contains(cursor) || under(&child.path, dragged) {
            continue;
        }
        let index = child.path[parent.len()];

        // Near the node's edge the drop targets the block around it,
        // keeping plain reordering reachable on any node.
        if !core_of(bounds).contains(cursor) {
            return Some(Target::Insert {
                index: if axis.of(cursor) < axis.of(bounds.center()) {
                    index
                } else {
                    index + 1
                },
                parent,
            });
        }

        // Always x, never `axis`: a chain runs one after the other,
        // and later is rightward however the block stacks.
        let core = core_of(bounds);
        let (wanted, before) = match (cursor.x - core.min.x)
            / core.width().max(1.0)
        {
            along if along < CHAIN_BAND => (Combinator::Chain, true),
            along if along > 1.0 - CHAIN_BAND => {
                (Combinator::Chain, false)
            }
            _ => (Combinator::All, false),
        };

        // The surrounding block already runs its children this way, so
        // a sibling is the same timing without the nesting.
        if wanted == combinator {
            return Some(Target::Insert {
                index: if before { index } else { index + 1 },
                parent,
            });
        }

        // A block that already runs its children this way would only
        // end up nested in an identical one, so the node joins it.
        if let Some(inner) = block_at(root, &child.path)
            && inner.combinator == wanted
        {
            return Some(Target::Insert {
                index: if before { 0 } else { inner.children.len() },
                parent: child.path.clone(),
            });
        }
        return Some(Target::Merge {
            path: child.path.clone(),
            combinator: wanted,
            before,
        });
    }

    // Loose in the block, past the children the cursor has cleared.
    // The dragged node still counts, so the index is into the tree as
    // it stands.
    let index = children
        .iter()
        .filter(|child| {
            axis.of(cursor) > axis.of(rect(child).center())
        })
        .count();
    Some(Target::Insert { parent, index })
}

/// The deepest block whose content area holds `cursor`. A block's
/// header strip belongs to its parent.
fn innermost_block(
    cursor: Vec2,
    layout: &[Placed],
    dragged: &[usize],
) -> Option<Vec<usize>> {
    layout
        .iter()
        .filter(|placed| {
            placed.label.is_some() && !under(&placed.path, dragged)
        })
        .filter(|placed| {
            let bounds = rect(placed);
            bounds.contains(cursor)
                && cursor.y >= bounds.min.y + HEADER_HEIGHT
        })
        .max_by_key(|placed| placed.path.len())
        .map(|placed| placed.path.clone())
}

/// The part of `bounds` a drop reads as the node itself; the margin
/// around it targets the enclosing block. Capped so even a narrow node
/// keeps a core.
fn core_of(bounds: Rect) -> Rect {
    let margin = Vec2::new(
        EDGE_MARGIN_PX.min(bounds.width() / 4.0),
        EDGE_MARGIN_PX.min(bounds.height() / 4.0),
    );
    Rect::from_corners(bounds.min + margin, bounds.max - margin)
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
    prune::empty_blocks(
        &mut editor_scene.edit().animation,
        &mut kept,
    );
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
    let moved = match target {
        Target::Insert { parent, index } => insert(
            &mut editor_scene.edit().animation,
            from,
            parent,
            *index,
        ),
        Target::Merge {
            path,
            combinator,
            before,
        } => merge(
            &mut editor_scene.edit().animation,
            from,
            path,
            combinator.clone(),
            *before,
        ),
    };
    let Some(landed) = moved else {
        return;
    };

    // After the move: pruning renumbers paths, and `target` was
    // resolved against the tree as it stood.
    let mut kept = selected_tail.map(|tail| {
        let mut path = landed;
        path.extend(tail);
        path
    });
    prune::empty_blocks(
        &mut editor_scene.edit().animation,
        &mut kept,
    );

    if let Some(mut selected) =
        world.get_resource_mut::<SelectedAction>()
        && selected.0.as_deref().is_some_and(|path| under(path, from))
    {
        selected.0 = kept;
    }
    RebuildTick::bump_in(world);
}

/// Moves `from` to `index` among `parent`'s children, returning where
/// it landed.
fn insert(
    root: &mut Block<Backend>,
    from: &[usize],
    parent: &[usize],
    index: usize,
) -> Option<Vec<usize>> {
    let (from_index, from_parent) = from.split_last()?;
    let node = take(root, from)?;

    let parent = after_removal(parent, from)?;
    // Removing the node closed the gap it left, so an index past it in
    // the same block is now one too far.
    let index = if parent == from_parent && index > *from_index {
        index - 1
    } else {
        index
    };

    let block = block_at_mut(root, &parent)?;
    let at = index.min(block.children.len());
    block.children.insert(at, node);

    let mut landed = parent;
    landed.push(at);
    Some(landed)
}

/// Wraps `from` and the node at `onto` in a new block, in `onto`'s
/// place, `from` first when `before`. Returns where `from` landed.
fn merge(
    root: &mut Block<Backend>,
    from: &[usize],
    onto: &[usize],
    combinator: Combinator,
    before: bool,
) -> Option<Vec<usize>> {
    let node = take(root, from)?;
    let onto = after_removal(onto, from)?;
    let (onto_index, onto_parent) = onto.split_last()?;

    let block = block_at_mut(root, onto_parent)?;
    if *onto_index >= block.children.len() {
        return None;
    }
    let host = block.children.remove(*onto_index);

    // Each delay stays on the node it belongs to; the wrapper has none.
    let children = if before {
        vec![node, host]
    } else {
        vec![host, node]
    };
    block.children.insert(
        *onto_index,
        SceneNode::Block {
            delay: None,
            block: Block {
                combinator,
                children,
                name: None,
            },
        },
    );

    let mut landed = onto_parent.to_vec();
    landed.push(*onto_index);
    landed.push(if before { 0 } else { 1 });
    Some(landed)
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

//
// Tree walking.
//

/// The block `path` names; `root` itself for an empty path.
fn block_at<'a>(
    root: &'a Block<Backend>,
    path: &[usize],
) -> Option<&'a Block<Backend>> {
    let mut block = root;
    for &index in path {
        let SceneNode::Block { block: inner, .. } =
            block.children.get(index)?
        else {
            return None;
        };
        block = inner;
    }
    Some(block)
}

/// The same walk as [`block_at`], mutable.
pub(super) fn block_at_mut<'a>(
    root: &'a mut Block<Backend>,
    path: &[usize],
) -> Option<&'a mut Block<Backend>> {
    let mut block = root;
    for &index in path {
        let SceneNode::Block { block: inner, .. } =
            block.children.get_mut(index)?
        else {
            return None;
        };
        block = inner;
    }
    Some(block)
}

fn axis_of(root: &Block<Backend>, path: &[usize]) -> Option<Axis> {
    Some(match block_at(root, path)?.combinator {
        Combinator::Chain => Axis::X,
        Combinator::All | Combinator::Flow(_) => Axis::Y,
    })
}

/// Whether `path` is `prefix` itself or sits under it.
pub(super) fn under(path: &[usize], prefix: &[usize]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == prefix[..]
}

fn is_child_of(path: &[usize], parent: &[usize]) -> bool {
    path.len() == parent.len() + 1
        && path[..parent.len()] == parent[..]
}

/// `placed`'s own rect.
pub(super) fn rect(placed: &Placed) -> Rect {
    Rect::new(
        placed.x,
        placed.y,
        placed.x + placed.w,
        placed.y + placed.h,
    )
}

//
// Drawing.
//

/// Tells the landing hint where `target` would land, or hides it when
/// there is nowhere. An insert draws the line; a merge outlines the
/// node it lands on, or the half the dragged node takes for a chain.
pub(super) fn announce_landing(
    commands: &mut Commands,
    theme: &EditorTheme,
    target: Option<&Target>,
    layout: &[Placed],
    root: &Block<Backend>,
) {
    let shown = match target {
        Some(Target::Insert { parent, index }) => {
            axis_of(root, parent)
                .and_then(|axis| {
                    line_rect(
                        parent,
                        *index,
                        layout,
                        axis,
                        theme.space.edge,
                    )
                })
                .map(ShowLanding::Insert)
        }
        Some(Target::Merge {
            path,
            combinator,
            before,
        }) => layout.iter().find(|placed| placed.path == *path).map(
            |placed| {
                merge_landing(
                    rect(placed),
                    combinator,
                    *before,
                    theme,
                )
            },
        ),
        None => None,
    };

    match shown {
        Some(show) => commands.trigger(show),
        None => commands.trigger(HideLanding),
    }
}

/// The outline for a merge onto `bounds`.
fn merge_landing(
    bounds: Rect,
    combinator: &Combinator,
    before: bool,
    theme: &EditorTheme,
) -> ShowLanding {
    let color = match combinator {
        Combinator::Chain => theme.palette.orange,
        Combinator::All | Combinator::Flow(_) => theme.palette.purple,
    };
    // A chain lands to one side, so the outline covers that half. An
    // overlap takes the whole node.
    let marked = if *combinator == Combinator::Chain {
        let mid = bounds.center().x;
        if before {
            Rect::new(bounds.min.x, bounds.min.y, mid, bounds.max.y)
        } else {
            Rect::new(mid, bounds.min.y, bounds.max.x, bounds.max.y)
        }
    } else {
        bounds
    };
    ShowLanding::Merge {
        bounds: marked,
        color,
    }
}

/// A `width`-thick band across `bounds`, centered on `at` along
/// `axis`.
fn band_across(
    bounds: Rect,
    axis: Axis,
    at: f32,
    width: f32,
) -> Rect {
    match axis {
        Axis::X => Rect::new(
            at - width / 2.0,
            bounds.min.y,
            at + width / 2.0,
            bounds.max.y,
        ),
        Axis::Y => Rect::new(
            bounds.min.x,
            at - width / 2.0,
            bounds.max.x,
            at + width / 2.0,
        ),
    }
}

/// The `width`-thick insert line at `index` among `parent`'s children:
/// centered in the gap between neighbours, flush against a lone
/// neighbour, or at the block's leading edge when there are none.
fn line_rect(
    parent: &[usize],
    index: usize,
    layout: &[Placed],
    axis: Axis,
    width: f32,
) -> Option<Rect> {
    let block =
        rect(layout.iter().find(|placed| placed.path == *parent)?);
    let content = Rect::new(
        block.min.x,
        block.min.y + HEADER_HEIGHT,
        block.max.x,
        block.max.y,
    );
    let children = layout
        .iter()
        .filter(|placed| is_child_of(&placed.path, parent))
        .map(rect)
        .collect::<Vec<_>>();

    let before =
        index.checked_sub(1).and_then(|index| children.get(index));
    let at = match (before, children.get(index)) {
        (Some(before), Some(after)) => {
            axis.of(before.max).midpoint(axis.of(after.min))
        }
        (None, Some(after)) => axis.of(after.min),
        (Some(before), None) => axis.of(before.max),
        (None, None) => axis.of(content.min),
    };
    Some(band_across(content, axis, at, width))
}

#[cfg(test)]
mod tests {
    use core::time::Duration;
    use std::collections::BTreeSet;

    use super::*;

    fn timed() -> SceneNode<Backend> {
        SceneNode::Draft {
            delay: None,
            duration: Duration::from_secs(1),
            name: None,
        }
    }

    fn delayed(secs: u64) -> SceneNode<Backend> {
        SceneNode::Draft {
            delay: Some(Duration::from_secs(secs)),
            duration: Duration::from_secs(1),
            name: None,
        }
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
        let mut root =
            combined(Combinator::All, vec![delayed(2), delayed(3)]);

        let landed =
            merge(&mut root, &[1], &[0], Combinator::Chain, false);

        assert_eq!(landed, Some(vec![0, 1]));
        let SceneNode::Block {
            delay: wrapper,
            block,
        } = &root.children[0]
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

    fn combined(
        combinator: Combinator,
        children: Vec<SceneNode<Backend>>,
    ) -> Block<Backend> {
        Block {
            combinator,
            children,
            name: None,
        }
    }

    /// Where a drop at `cursor` lands with `[1]` being dragged.
    fn drop_at(
        root: &Block<Backend>,
        cursor: Vec2,
    ) -> Option<Target> {
        let layout = block_layout::layout(
            root,
            TimelineView::default(),
            &BTreeSet::new(),
        );
        resolve(cursor, &layout, root, &[1])
    }

    #[test]
    fn a_chain_edge_joins_a_chain_block_instead_of_nesting_one() {
        let root = combined(
            Combinator::All,
            vec![
                SceneNode::block(combined(
                    Combinator::Chain,
                    vec![timed(), timed()],
                )),
                timed(),
            ],
        );

        assert_eq!(
            drop_at(&root, Vec2::new(40.0, 36.0)),
            Some(Target::Insert {
                parent: vec![0],
                index: 0
            })
        );
        assert_eq!(
            drop_at(&root, Vec2::new(280.0, 36.0)),
            Some(Target::Insert {
                parent: vec![0],
                index: 2
            })
        );
    }

    #[test]
    fn an_overlap_joins_an_all_block_instead_of_nesting_one() {
        let root = combined(
            Combinator::Chain,
            vec![
                SceneNode::block(combined(
                    Combinator::All,
                    vec![timed(), timed()],
                )),
                timed(),
            ],
        );

        assert_eq!(
            drop_at(&root, Vec2::new(80.0, 36.0)),
            Some(Target::Insert {
                parent: vec![0],
                index: 2
            })
        );
    }

    #[test]
    fn a_chain_edge_merges_into_a_block_it_cannot_join() {
        let merged = Some(Target::Merge {
            path: vec![0],
            combinator: Combinator::Chain,
            before: true,
        });
        // An `All` block doesn't run its children in a chain, and a
        // `Flow` block never matches what a drop asks for.
        for (inner, x) in [
            (Combinator::All, 20.0),
            (Combinator::Flow(Duration::from_millis(500)), 40.0),
        ] {
            let root = combined(
                Combinator::All,
                vec![
                    SceneNode::block(combined(
                        inner.clone(),
                        vec![timed(), timed()],
                    )),
                    timed(),
                ],
            );

            assert_eq!(
                drop_at(&root, Vec2::new(x, 36.0)),
                merged,
                "into {inner:?}"
            );
        }
    }
}
