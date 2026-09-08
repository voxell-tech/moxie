//! Dragging a node's body elsewhere in the tree: between two of a
//! block's children, or onto another node to wrap the pair in a new
//! block. Action leaves and block headers share this, each just a node
//! at a path.
//!
//! The tree is written only when the drag ends, and nothing on screen
//! moves while it runs. One layout, taken at drag start, describes the
//! whole gesture; every move is a hit test against it.

use bevy::feathers::cursor::{EntityCursor, OverrideCursor};
use bevy::picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{UiGlobalTransform, UiScale};
use bevy::window::SystemCursorIcon;
use bevy_fynix::{BevyFynix, WorldEntityMut};
use bevy_motiongfx::scene::backend::Backend;
use fynix::element::Element;
use fynix::ui::ElementMut;
use motiongfx_scene::block::{Block, Combinator, Node as SceneNode};
use moxie_ui::layout::logical_rect;
use moxie_ui::reactive::FynixHost;
use moxie_ui::theme::EditorTheme;

use super::{BlockFoldState, RebuildTick};
use crate::block_layout::{self, HEADER_HEIGHT, Placed};
use crate::{EditorScene, TimelineView};

/// How close to a node's own edge a drop stops being about that node
/// and starts being about the block around it.
const EDGE_MARGIN_PX: f32 = 8.0;
/// How much of a node's core, at either end, chains rather than
/// overlaps.
const CHAIN_BAND: f32 = 0.25;
/// Thickness of the insertion line.
const LINE_PX: f32 = 2.0;
/// How far the merge outline sits outside the node it marks.
const OUTLINE_GROW: f32 = 2.0;
/// Cursor shown for the duration of a drag.
const GRABBING: EntityCursor =
    EntityCursor::System(SystemCursorIcon::Grabbing);

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
    entity: Entity,
    path: Vec<usize>,
    /// Every box as it stood when the drag started.
    layout: Vec<Placed>,
    /// Subtracted from a pointer position to reach the local space the
    /// `Placed`s are in, scroll included.
    conversion_offset: Vec2,
    /// Added to a `Placed` position to reach `TrackArea`'s space, where
    /// the visuals are laid out.
    to_area: Vec2,
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

/// What a drag draws: the ghost of the dragged node and the landing
/// hints.
#[derive(Resource)]
pub(crate) struct Visuals {
    area: Entity,
    ghost: Entity,
    ghost_label: Entity,
    line: Entity,
    outline: Entity,
}

impl Visuals {
    pub(super) fn new(
        area: Entity,
        ghost: Entity,
        ghost_label: Entity,
        line: Entity,
        outline: Entity,
    ) -> Self {
        Self {
            area,
            ghost,
            ghost_label,
            line,
            outline,
        }
    }
}

/// Makes `handle` a node's own body: dragging it moves `path`
/// elsewhere in the tree. `fill`/`border`/`label` are `path`'s own, so
/// the floating ghost reads as the same box.
pub(crate) fn body<'r, 'u, 'a, E: Element<FynixHost>>(
    handle: &'r mut ElementMut<'u, 'a, FynixHost, E>,
    path: Vec<usize>,
    fill: Color,
    border: Color,
    label: String,
) -> &'r mut ElementMut<'u, 'a, FynixHost, E> {
    // Not `event_target()`: neither `Label` nor `Icon` ignores the
    // pointer, so a grab on the text or the chevron reports that
    // child instead of the box being wired here.
    let entity = handle.id();
    handle
        .observe(
            move |start: On<Pointer<DragStart>>,
                  scale: Res<UiScale>,
                  view: Res<TimelineView>,
                  kernel: Res<BevyFynix<EditorTheme>>,
                  computed: Query<(
                &ComputedNode,
                &UiGlobalTransform,
            )>,
                  editor_scene: Res<EditorScene>,
                  folded: Res<BlockFoldState>,
                  visuals: Res<Visuals>,
                  mut dragging: ResMut<Dragging>,
                  mut visibility: Query<&mut Visibility>,
                  mut nodes: Query<&mut Node>,
                  mut backgrounds: Query<&mut BackgroundColor>,
                  mut borders: Query<&mut BorderColor>,
                  mut texts: Query<&mut Text>,
                  mut text_colors: Query<&mut TextColor>,
                  mut override_cursor: ResMut<OverrideCursor>| {
                if start.button != PointerButton::Primary
                    || path.is_empty()
                {
                    // The root has nowhere to land.
                    return;
                }
                let Ok((node, transform)) = computed.get(entity)
                else {
                    return;
                };
                let window = logical_rect(node, transform);

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

                // The dragged box against where the layout put it,
                // giving the scroll without reading the `ScrollArea`.
                let conversion_offset = window.min - origin.min;
                let cursor = start.pointer_location.position
                    / scale.0
                    - conversion_offset;

                let Ok((area, area_transform)) =
                    computed.get(visuals.area)
                else {
                    return;
                };
                let to_area = conversion_offset
                    - logical_rect(area, area_transform).min;

                show_ghost(
                    &mut nodes,
                    &mut backgrounds,
                    &mut borders,
                    &mut texts,
                    &mut text_colors,
                    &visuals,
                    origin,
                    to_area,
                    fill,
                    border,
                    label.clone(),
                    kernel.theme().color.text.with_alpha(0.9),
                );
                if let Ok(mut visibility) = visibility.get_mut(entity)
                {
                    *visibility = Visibility::Hidden;
                }
                // Held for the whole gesture so nothing the pointer
                // crosses, a resize handle above all, swaps the cursor.
                override_cursor.0 = Some(GRABBING);

                dragging.0 = Some(Gesture {
                    entity,
                    path: path.clone(),
                    layout,
                    conversion_offset,
                    to_area,
                    grab_offset: cursor - origin.min,
                    target: None,
                });
            },
        )
        .observe(
            move |drag: On<Pointer<Drag>>,
                  scale: Res<UiScale>,
                  kernel: Res<BevyFynix<EditorTheme>>,
                  editor_scene: Res<EditorScene>,
                  visuals: Res<Visuals>,
                  mut dragging: ResMut<Dragging>,
                  mut nodes: Query<&mut Node>,
                  mut backgrounds: Query<&mut BackgroundColor>,
                  mut borders: Query<&mut BorderColor>| {
                let Some(gesture) = &mut dragging.0 else {
                    return;
                };
                let cursor = drag.pointer_location.position / scale.0
                    - gesture.conversion_offset;

                if let Ok(mut ghost) = nodes.get_mut(visuals.ghost) {
                    let at = cursor - gesture.grab_offset
                        + gesture.to_area;
                    ghost.left = px(at.x);
                    ghost.top = px(at.y);
                }

                let root = &editor_scene.scene().0.animation;
                gesture.target = resolve(
                    cursor,
                    &gesture.layout,
                    root,
                    &gesture.path,
                );
                show_landing(
                    &mut nodes,
                    &mut backgrounds,
                    &mut borders,
                    &visuals,
                    kernel.theme(),
                    gesture.target.as_ref(),
                    &gesture.layout,
                    root,
                    gesture.to_area,
                );
            },
        )
}

/// Ends the `body` drag in progress: clears the ghost and commits the
/// drop, unless it settled where it started.
///
/// Global, not one observer per box: a child
/// [`Button`](bevy::ui_widgets::Button) (the fold chevron among them)
/// stops `DragEnd` propagating and would otherwise strand the gesture
/// until Escape.
pub(crate) fn on_drag_end(
    _: On<Pointer<DragEnd>>,
    visuals: Option<Res<Visuals>>,
    mut dragging: ResMut<Dragging>,
    mut visibility: Query<&mut Visibility>,
    mut nodes: Query<&mut Node>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    let Some(gesture) = dragging.0.take() else {
        return;
    };
    let Some(visuals) = visuals else {
        return;
    };
    hide(&mut nodes, &visuals);
    release_cursor(&mut override_cursor);
    if let Ok(mut visibility) = visibility.get_mut(gesture.entity) {
        *visibility = Visibility::Inherited;
    }

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
    mut visibility: Query<&mut Visibility>,
    mut nodes: Query<&mut Node>,
    mut override_cursor: ResMut<OverrideCursor>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Some(gesture) = dragging.0.take() else {
        return;
    };
    hide(&mut nodes, &visuals);
    release_cursor(&mut override_cursor);
    if let Ok(mut visibility) = visibility.get_mut(gesture.entity) {
        *visibility = Visibility::Inherited;
    }
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
    // The dragged node still counts though its box is hidden, so the
    // index is into the tree as it stands.
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

/// Writes the drop's result back into the scene.
fn commit(world: &mut World, from: &[usize], target: &Target) {
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
    if moved.is_none() {
        return;
    }
    // After the move: pruning renumbers paths, and `target` was
    // resolved against the tree as it stood.
    prune_empty(&mut editor_scene.edit().0.animation);
    if let Some(mut tick) = world.get_resource_mut::<RebuildTick>() {
        tick.0 = tick.0.wrapping_add(1);
    }
}

/// Moves `from` to `index` among `parent`'s children.
fn insert(
    root: &mut Block<Backend>,
    from: &[usize],
    parent: &[usize],
    index: usize,
) -> Option<()> {
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
    block.children.insert(index.min(block.children.len()), node);
    Some(())
}

/// Wraps `from` and the node at `onto` in a new block, in `onto`'s
/// place, `from` first when `before`.
fn merge(
    root: &mut Block<Backend>,
    from: &[usize],
    onto: &[usize],
    combinator: Combinator,
    before: bool,
) -> Option<()> {
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
    Some(())
}

/// Drops every empty block, innermost first so one emptied by losing
/// its last nested block goes too. The root stays, empty or not.
fn prune_empty(block: &mut Block<Backend>) {
    block.children.retain_mut(|child| {
        let SceneNode::Block { block, .. } = child else {
            return true;
        };
        prune_empty(block);
        !block.children.is_empty()
    });
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

/// The floating copy of the node being dragged, hidden until a drag
/// shows it.
pub(super) fn hidden_ghost() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            padding: UiRect::new(px(4), Val::ZERO, px(2), Val::ZERO),
            overflow: Overflow::clip(),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        GlobalZIndex(200),
        Pickable::IGNORE,
    )
}

/// The ghost's label, blank until a drag shows it.
pub(super) fn hidden_ghost_label() -> impl Bundle {
    (
        Text::new(String::new()),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::NONE),
        TextLayout::linebreak(LineBreak::NoWrap),
        Pickable::IGNORE,
    )
}

/// The line marking where an insert would land, hidden until a drag
/// shows it.
pub(super) fn hidden_line(color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            ..default()
        },
        BackgroundColor(color),
        GlobalZIndex(150),
        Pickable::IGNORE,
    )
}

/// The outline marking the node a merge would absorb, hidden until a
/// drag shows it.
pub(super) fn hidden_outline(color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            border: UiRect::all(px(2)),
            ..default()
        },
        BackgroundColor(color.with_alpha(0.15)),
        BorderColor::all(color),
        GlobalZIndex(150),
        Pickable::IGNORE,
    )
}

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
            let Some(bounds) =
                line_rect(parent, *index, layout, axis)
            else {
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

/// A [`LINE_PX`] band across `bounds`, centered on `at` along `axis`.
fn band_across(bounds: Rect, axis: Axis, at: f32) -> Rect {
    match axis {
        Axis::X => Rect::new(
            at - LINE_PX / 2.0,
            bounds.min.y,
            at + LINE_PX / 2.0,
            bounds.max.y,
        ),
        Axis::Y => Rect::new(
            bounds.min.x,
            at - LINE_PX / 2.0,
            bounds.max.x,
            at + LINE_PX / 2.0,
        ),
    }
}

/// The insert line at `index` among `parent`'s children: centered in
/// the gap between neighbours, flush against a lone neighbour, or at
/// the block's leading edge when there are none.
fn line_rect(
    parent: &[usize],
    index: usize,
    layout: &[Placed],
    axis: Axis,
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
    Some(band_across(content, axis, at))
}

/// Reveals and places the ghost at `origin`, reading as `label` in
/// `fill`/`border`.
#[expect(
    clippy::too_many_arguments,
    reason = "one write per component the ghost paints itself with"
)]
fn show_ghost(
    nodes: &mut Query<&mut Node>,
    backgrounds: &mut Query<&mut BackgroundColor>,
    borders: &mut Query<&mut BorderColor>,
    texts: &mut Query<&mut Text>,
    text_colors: &mut Query<&mut TextColor>,
    visuals: &Visuals,
    origin: Rect,
    to_area: Vec2,
    fill: Color,
    border: Color,
    label: String,
    text_color: Color,
) {
    if let Ok(mut node) = nodes.get_mut(visuals.ghost) {
        node.display = Display::Flex;
        node.left = px(origin.min.x + to_area.x);
        node.top = px(origin.min.y + to_area.y);
        node.width = px(origin.width());
        node.height = px(origin.height());
    }
    if let Ok(mut background) = backgrounds.get_mut(visuals.ghost) {
        background.0 = fill;
    }
    if let Ok(mut border_color) = borders.get_mut(visuals.ghost) {
        *border_color = BorderColor::all(border);
    }
    if let Ok(mut text) = texts.get_mut(visuals.ghost_label) {
        text.0 = label;
    }
    if let Ok(mut color) = text_colors.get_mut(visuals.ghost_label) {
        color.0 = text_color;
    }
}

fn hide_landing(nodes: &mut Query<&mut Node>, visuals: &Visuals) {
    for entity in [visuals.line, visuals.outline] {
        if let Ok(mut node) = nodes.get_mut(entity) {
            node.display = Display::None;
        }
    }
}

/// Drops the drag-wide cursor, if it's the one this set.
fn release_cursor(cursor: &mut OverrideCursor) {
    if cursor.0 == Some(GRABBING) {
        cursor.0 = None;
    }
}

/// Hides the ghost and both landing hints.
fn hide(nodes: &mut Query<&mut Node>, visuals: &Visuals) {
    hide_landing(nodes, visuals);
    if let Ok(mut node) = nodes.get_mut(visuals.ghost) {
        node.display = Display::None;
    }
}
