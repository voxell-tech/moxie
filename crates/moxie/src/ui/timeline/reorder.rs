//! Dragging a node's body elsewhere in the tree: between two of a
//! block's children, or onto another node to wrap the pair in a new
//! block. Action leaves and block headers share this, each just a node
//! at a path.
//!
//! The tree is written only when the drag ends. Until then the dragged
//! box (and its subtree) is the preview, offset to follow the cursor
//! while the rest of the layout keeps flowing under it. [`preview`]
//! runs every frame, not just on pointer motion, so a pan or a zoom
//! mid-drag stays in step. A slim line or an outline marks where a
//! release would land.

use bevy::feathers::cursor::OverrideCursor;
use bevy::picking::events::{DragEnd, DragStart, Pointer};
use bevy::picking::pointer::{PointerButton, PointerLocation};
use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform, UiScale};
use bevy_fynix::{BevyFynix, WorldEntityMut};
use bevy_motiongfx::scene::backend::Backend;
use fynix::element::Element;
use fynix::ui::ElementMut;
use motiongfx_scene::block::{Block, Combinator, Node as SceneNode};
use moxie_ui::drag::{grab, ungrab};
use moxie_ui::layout::logical_rect;
use moxie_ui::reactive::FynixHost;
use moxie_ui::theme::EditorTheme;

use super::retime::{BoxPath, GapPath};
use super::{BlockFoldState, RebuildTick, TrackViewport};
use crate::block_layout::{self, HEADER_HEIGHT, Placed};
use crate::{EditorScene, SelectedAction, TimelineView};

/// How close to a node's own edge a drop stops being about that node
/// and starts being about the block around it.
const EDGE_MARGIN_PX: f32 = 8.0;
/// How much of a node's core, at either end, chains rather than
/// overlaps.
const CHAIN_BAND: f32 = 0.25;
/// How far the merge outline sits outside the node it marks.
const OUTLINE_GROW: f32 = 2.0;

/// The node being dragged, if any.
#[derive(Resource, Default)]
pub(crate) struct Dragging(Option<Gesture>);

/// Where a dragged node lands when released.
#[derive(Clone, PartialEq)]
enum Target {
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
    /// Subtracted from the cursor, in content space, for the box's
    /// top-left.
    grab_offset: Vec2,
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

/// The landing hints, both children of `area` (the track area frame)
/// so they outlive the box list's rebuilds.
#[derive(Resource)]
pub(crate) struct Visuals {
    area: Entity,
    line: Entity,
    outline: Entity,
}

impl Visuals {
    pub(super) fn new(
        area: Entity,
        line: Entity,
        outline: Entity,
    ) -> Self {
        Self {
            area,
            line,
            outline,
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
              view: Res<TimelineView>,
              editor_scene: Res<EditorScene>,
              folded: Res<BlockFoldState>,
              q_viewport: Query<
            (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
            With<TrackViewport>,
        >,
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

            let cursor = start.pointer_location.position / scale.0;
            let content = to_content(cursor, node, transform, scroll);

            let layout = block_layout::layout(
                &editor_scene.scene().0.animation,
                *view,
                folded.paths(),
            );
            let Some(origin) =
                layout.iter().find(|p| p.path == path).map(rect)
            else {
                return;
            };

            grab(&mut override_cursor);
            dragging.0 = Some(Gesture {
                path: path.clone(),
                grab_offset: content - origin.min,
                target: None,
            });
        },
    )
}

/// The mouse pointer in logical screen space, if it has a location.
fn cursor(
    pointers: &Query<&PointerLocation>,
    scale: &UiScale,
) -> Option<Vec2> {
    pointers
        .iter()
        .find_map(|pointer| pointer.location())
        .map(|location| location.position / scale.0)
}

/// The per-frame drag: lays the tree back out against the current
/// view, drags the box under the cursor, and marks where a release
/// would land. Runs every frame, not just on pointer motion, so a pan
/// or a zoom mid-drag stays in step.
pub(crate) fn preview(
    kernel: Res<BevyFynix<EditorTheme>>,
    scale: Res<UiScale>,
    editor_scene: Res<EditorScene>,
    folded: Res<BlockFoldState>,
    visuals: Option<Res<Visuals>>,
    view: Res<TimelineView>,
    mut dragging: ResMut<Dragging>,
    pointers: Query<&PointerLocation>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    q_area: Query<(&ComputedNode, &UiGlobalTransform)>,
    q_boxes: Query<(Entity, &BoxPath)>,
    q_gaps: Query<(Entity, &GapPath), Without<BoxPath>>,
    mut nodes: Query<&mut Node>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
    mut commands: Commands,
) {
    let (Some(gesture), Some(visuals)) =
        (dragging.0.as_mut(), visuals)
    else {
        return;
    };
    let Some(cursor) = cursor(&pointers, &scale) else {
        return;
    };
    let Ok((vp_node, vp_transform, scroll)) = q_viewport.single()
    else {
        return;
    };
    let Ok((area_node, area_transform)) = q_area.get(visuals.area)
    else {
        return;
    };
    let vp_rect = logical_rect(vp_node, vp_transform);
    let drag_z = kernel.theme().layer.drag;

    let content = Vec2::new(
        cursor.x - vp_rect.min.x,
        cursor.y - vp_rect.min.y + scroll.y,
    );
    let root = &editor_scene.scene().0.animation;
    let layout = block_layout::layout(root, *view, folded.paths());

    // The whole subtree moves rigidly: every box and gap under the
    // dragged path shifts by the same delta, so a block carries its
    // children rather than sliding out of its own border.
    let Some(origin) = layout
        .iter()
        .find(|placed| placed.path == gesture.path)
        .map(rect)
    else {
        return;
    };
    let delta = (content - gesture.grab_offset) - origin.min;
    for (entity, box_path) in &q_boxes {
        if !under(&box_path.0, &gesture.path) {
            continue;
        }
        commands.entity(entity).insert(GlobalZIndex(drag_z));
        if let Some(placed) =
            layout.iter().find(|p| p.path == box_path.0)
            && let Ok(mut node) = nodes.get_mut(entity)
        {
            node.left = px(placed.x + delta.x);
            node.top = px(placed.y + delta.y);
        }
    }
    for (entity, gap_path) in &q_gaps {
        if !under(&gap_path.0, &gesture.path) {
            continue;
        }
        commands.entity(entity).insert(GlobalZIndex(drag_z));
        if let Some(Placed {
            gap_x: Some(gap_x),
            y,
            ..
        }) = layout.iter().find(|p| p.path == gap_path.0)
            && let Ok(mut node) = nodes.get_mut(entity)
        {
            node.left = px(gap_x + delta.x);
            node.top = px(y + delta.y);
        }
    }

    gesture.target = resolve(content, &layout, root, &gesture.path);
    let to_area = Vec2::new(
        vp_rect.min.x - logical_rect(area_node, area_transform).min.x,
        vp_rect.min.y
            - scroll.y
            - logical_rect(area_node, area_transform).min.y,
    );
    show_landing(
        &mut nodes,
        &mut backgrounds,
        &mut borders,
        &visuals,
        kernel.theme(),
        gesture.target.as_ref(),
        &layout,
        root,
        to_area,
    );
}

/// A cursor in logical screen space, mapped into the viewport's
/// content space, where the `Placed`s live.
fn to_content(
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
    commands.queue(|world: &mut World| {
        if let Some(mut tick) =
            world.get_resource_mut::<RebuildTick>()
        {
            tick.0 = tick.0.wrapping_add(1);
        }
    });
}

/// Ends the `body` drag in progress and commits the drop, unless it
/// settled where it started.
///
/// Global, not one observer per box: a child
/// [`Button`](bevy::ui_widgets::Button) (the fold chevron among them)
/// stops `DragEnd` propagating and would otherwise strand the gesture
/// until Escape.
pub(crate) fn on_drag_end(
    _: On<Pointer<DragEnd>>,
    visuals: Option<Res<Visuals>>,
    mut dragging: ResMut<Dragging>,
    mut nodes: Query<&mut Node>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    let Some(gesture) = dragging.0.take() else {
        return;
    };
    if let Some(visuals) = &visuals {
        hide_landing(&mut nodes, visuals);
    }
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
pub(crate) fn cancel_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    visuals: Res<Visuals>,
    mut dragging: ResMut<Dragging>,
    mut nodes: Query<&mut Node>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if dragging.0.take().is_none() {
        return;
    }
    hide_landing(&mut nodes, &visuals);
    end_drag(&mut override_cursor, &mut commands);
}

//
// Where a release lands.
//

/// Where releasing at `cursor` would put the node at `dragged`.
fn resolve(
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
            &mut editor_scene.edit().0.animation,
            from,
            parent,
            *index,
        ),
        Target::Merge {
            path,
            combinator,
            before,
        } => merge(
            &mut editor_scene.edit().0.animation,
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
    prune_empty(&mut editor_scene.edit().0.animation, &mut kept);

    if let Some(mut selected) =
        world.get_resource_mut::<SelectedAction>()
        && selected.0.as_deref().is_some_and(|path| under(path, from))
    {
        selected.0 = kept;
    }
    if let Some(mut tick) = world.get_resource_mut::<RebuildTick>() {
        tick.0 = tick.0.wrapping_add(1);
    }
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
    let mut node = take(root, from)?;
    let onto = after_removal(onto, from)?;
    let (onto_index, onto_parent) = onto.split_last()?;

    let block = block_at_mut(root, onto_parent)?;
    if *onto_index >= block.children.len() {
        return None;
    }
    let mut host = block.children.remove(*onto_index);

    // The pair starts where the replaced node did: its delay moves out
    // to the wrapper, and neither child keeps a head start.
    let delay = delay_of(&mut host).take();
    *delay_of(&mut node) = None;

    let children = if before {
        vec![node, host]
    } else {
        vec![host, node]
    };
    block.children.insert(
        *onto_index,
        SceneNode::Block {
            delay,
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

/// Drops every empty block, innermost first so one emptied by losing
/// its last nested block goes too. The root stays, empty or not.
/// `keep` is a path to carry through the renumbering, cleared if it
/// pointed inside a pruned block.
fn prune_empty(
    block: &mut Block<Backend>,
    keep: &mut Option<Vec<usize>>,
) {
    let mut i = 0;
    while i < block.children.len() {
        let SceneNode::Block { block: inner, .. } =
            &mut block.children[i]
        else {
            i += 1;
            continue;
        };

        let mut inner_keep = match keep.as_deref() {
            Some([first, rest @ ..]) if *first == i => {
                Some(rest.to_vec())
            }
            _ => None,
        };
        prune_empty(inner, &mut inner_keep);
        if keep.as_deref().and_then(<[usize]>::first) == Some(&i) {
            match inner_keep {
                Some(rest) => {
                    let k = keep.as_mut().unwrap();
                    k.truncate(1);
                    k.extend(rest);
                }
                None => *keep = None,
            }
        }

        if inner.children.is_empty() {
            block.children.remove(i);
            match keep.as_deref() {
                Some([first, ..]) if *first == i => *keep = None,
                Some([first, ..]) if *first > i => {
                    keep.as_mut().unwrap()[0] -= 1;
                }
                _ => {}
            }
        } else {
            i += 1;
        }
    }
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
fn block_at_mut<'a>(
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

fn delay_of(
    node: &mut SceneNode<Backend>,
) -> &mut Option<core::time::Duration> {
    match node {
        SceneNode::Block { delay, .. }
        | SceneNode::Action { delay, .. }
        | SceneNode::Draft { delay, .. } => delay,
    }
}

/// Whether `path` is `prefix` itself or sits under it.
fn under(path: &[usize], prefix: &[usize]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == prefix[..]
}

fn is_child_of(path: &[usize], parent: &[usize]) -> bool {
    path.len() == parent.len() + 1
        && path[..parent.len()] == parent[..]
}

/// `placed`'s own rect.
fn rect(placed: &Placed) -> Rect {
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

/// Shows the landing hints `target` calls for and hides the rest. An
/// insert draws the line; a merge outlines the node it lands on, or
/// the half the dragged node takes for a chain.
fn show_landing(
    nodes: &mut Query<&mut Node>,
    backgrounds: &mut Query<&mut BackgroundColor>,
    borders: &mut Query<&mut BorderColor>,
    visuals: &Visuals,
    theme: &EditorTheme,
    target: Option<&Target>,
    layout: &[Placed],
    root: &Block<Backend>,
    to_area: Vec2,
) {
    hide_landing(nodes, visuals);
    match target {
        Some(Target::Insert { parent, index }) => {
            let Some(axis) = axis_of(root, parent) else {
                return;
            };
            let Some(bounds) = line_rect(
                parent,
                *index,
                layout,
                axis,
                theme.space.edge,
            ) else {
                return;
            };
            place(nodes, visuals.line, bounds, to_area);
        }
        Some(Target::Merge {
            path,
            combinator,
            before,
        }) => {
            let Some(bounds) = layout
                .iter()
                .find(|placed| placed.path == *path)
                .map(rect)
            else {
                return;
            };
            let color = match combinator {
                Combinator::Chain => theme.palette.orange,
                Combinator::All | Combinator::Flow(_) => {
                    theme.palette.purple
                }
            };
            // A chain lands to one side, so the outline covers that
            // half. An overlap takes the whole node.
            let marked = if *combinator == Combinator::Chain {
                let mid = bounds.center().x;
                if *before {
                    Rect::new(
                        bounds.min.x,
                        bounds.min.y,
                        mid,
                        bounds.max.y,
                    )
                } else {
                    Rect::new(
                        mid,
                        bounds.min.y,
                        bounds.max.x,
                        bounds.max.y,
                    )
                }
            } else {
                bounds
            };

            paint(backgrounds, borders, visuals.outline, color);
            place(
                nodes,
                visuals.outline,
                marked.inflate(OUTLINE_GROW),
                to_area,
            );
        }
        None => {}
    }
}

/// Reveals `entity` at `bounds`, offset by `to_area` into the visuals'
/// space.
fn place(
    nodes: &mut Query<&mut Node>,
    entity: Entity,
    bounds: Rect,
    to_area: Vec2,
) {
    if let Ok(mut node) = nodes.get_mut(entity) {
        node.display = Display::Flex;
        node.left = px(bounds.min.x + to_area.x);
        node.top = px(bounds.min.y + to_area.y);
        node.width = px(bounds.width());
        node.height = px(bounds.height());
    }
}

/// Recolors `entity`.
fn paint(
    backgrounds: &mut Query<&mut BackgroundColor>,
    borders: &mut Query<&mut BorderColor>,
    entity: Entity,
    color: Color,
) {
    if let Ok(mut background) = backgrounds.get_mut(entity) {
        background.0 = color.with_alpha(0.15);
    }
    if let Ok(mut border) = borders.get_mut(entity) {
        *border = BorderColor::all(color);
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

/// Hides both landing hints.
fn hide_landing(nodes: &mut Query<&mut Node>, visuals: &Visuals) {
    for entity in [visuals.line, visuals.outline] {
        if let Ok(mut node) = nodes.get_mut(entity) {
            node.display = Display::None;
        }
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use motiongfx_scene::block::{Block, Node as SceneNode};

    use super::*;

    fn leaf() -> SceneNode<Backend> {
        SceneNode::Draft {
            delay: None,
            duration: Duration::ZERO,
            name: None,
        }
    }

    fn block(
        children: Vec<SceneNode<Backend>>,
    ) -> SceneNode<Backend> {
        SceneNode::block(Block::chain(children))
    }

    #[test]
    fn prune_shifts_kept_past_a_removed_sibling() {
        let mut root = Block::chain(vec![block(vec![]), leaf()]);
        let mut keep = Some(vec![1]);
        prune_empty(&mut root, &mut keep);
        assert_eq!(keep, Some(vec![0]));
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn prune_leaves_kept_before_a_removed_sibling() {
        let mut root = Block::chain(vec![leaf(), block(vec![])]);
        let mut keep = Some(vec![0]);
        prune_empty(&mut root, &mut keep);
        assert_eq!(keep, Some(vec![0]));
    }

    #[test]
    fn prune_clears_kept_inside_a_removed_block() {
        let mut root =
            Block::chain(vec![block(vec![block(vec![])]), leaf()]);
        let mut keep = Some(vec![0, 0]);
        prune_empty(&mut root, &mut keep);
        assert_eq!(keep, None);
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn prune_rebases_kept_in_a_surviving_nested_block() {
        let mut root = Block::chain(vec![
            block(vec![block(vec![]), leaf()]),
            leaf(),
        ]);
        let mut keep = Some(vec![0, 1]);
        prune_empty(&mut root, &mut keep);
        assert_eq!(keep, Some(vec![0, 0]));
    }
}
