//! Where a node dropped on the track lands, and putting it there:
//! between two of a block's children, or onto another node to wrap
//! the pair in a new block. Moving a node and creating one from a
//! field both land this way.

use bevy::prelude::*;
use bevy_motiongfx::scene::backend::Backend;
use motiongfx_scene::block::{Block, Combinator, Node as SceneNode};
use moxie_ui::theme::EditorTheme;

use super::block_layout::{Placed, header_height};
use super::hint::{HintNode, Shape};
use crate::scene::is_track;

/// How close to a node's own edge a drop stops being about that node
/// and starts being about the block around it.
const EDGE_MARGIN_PX: f32 = 8.0;
/// How much of a node's core, at either end, chains rather than
/// overlaps.
const CHAIN_BAND: f32 = 0.25;

/// Where a dropped node lands.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    /// Among `parent`'s children, at `index`.
    Insert { parent: Vec<usize>, index: usize },
    /// Onto `path`, wrapping the two of them in a new block under
    /// `combinator`. `before` puts the dropped node first.
    Merge {
        path: Vec<usize>,
        combinator: Combinator,
        before: bool,
    },
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

/// Where dropping at `cursor` would land. `dragged` is the node being
/// moved, if one is: nothing lands on or inside it. Only tracks sit
/// under the root, so a drop that would land there lands at the end
/// of the track whose lane holds the cursor instead.
pub(super) fn resolve(
    cursor: Vec2,
    layout: &[Placed],
    root: &Block<Backend>,
    dragged: Option<&[usize]>,
) -> Option<Target> {
    let resolved = resolve_in(cursor, layout, root, dragged);
    let on_root = match &resolved {
        None => true,
        Some(Target::Insert { parent, .. }) => parent.is_empty(),
        Some(Target::Merge { path, .. }) => is_track(path),
    };
    if !on_root {
        return resolved;
    }
    into_track(cursor, layout)
}

/// The lane's track nearest `cursor` vertically, the cursor's place
/// among its children chosen along the time axis.
fn into_track(cursor: Vec2, layout: &[Placed]) -> Option<Target> {
    let tracks = layout
        .iter()
        .filter(|placed| {
            placed.label.is_some() && is_track(&placed.path)
        })
        .collect::<Vec<_>>();
    let track = tracks
        .iter()
        .rev()
        .find(|track| track.y <= cursor.y)
        .or(tracks.first())?;
    let index = layout
        .iter()
        .filter(|placed| is_child_of(&placed.path, &track.path))
        .filter(|child| cursor.x > rect(child).center().x)
        .count();
    Some(Target::Insert {
        parent: track.path.clone(),
        index,
    })
}

/// [`resolve`] without the root's special case.
fn resolve_in(
    cursor: Vec2,
    layout: &[Placed],
    root: &Block<Backend>,
    dragged: Option<&[usize]>,
) -> Option<Target> {
    let is_dragged = |path: &[usize]| {
        dragged.is_some_and(|dragged| under(path, dragged))
    };
    let parent = innermost_block(cursor, layout, is_dragged)?;
    let axis = axis_of(root, &parent)?;
    let children = layout
        .iter()
        .filter(|placed| is_child_of(&placed.path, &parent))
        .collect::<Vec<_>>();

    let combinator = block_at(root, &parent)?.combinator.clone();

    for child in &children {
        let bounds = rect(child);
        if !bounds.contains(cursor) || is_dragged(&child.path) {
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

        // The surrounding block already runs its children this way,
        // so a sibling is the same timing without the
        // nesting.
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
    is_dragged: impl Fn(&[usize]) -> bool,
) -> Option<Vec<usize>> {
    layout
        .iter()
        .filter(|placed| {
            placed.label.is_some() && !is_dragged(&placed.path)
        })
        .filter(|placed| {
            let bounds = rect(placed);
            bounds.contains(cursor)
                && cursor.y
                    >= bounds.min.y + header_height(&placed.path)
        })
        .max_by_key(|placed| placed.path.len())
        .map(|placed| placed.path.clone())
}

/// The part of `bounds` a drop reads as the node itself; the margin
/// around it targets the enclosing block. Capped so even a narrow
/// node keeps a core.
fn core_of(bounds: Rect) -> Rect {
    let margin = Vec2::new(
        EDGE_MARGIN_PX.min(bounds.width() / 4.0),
        EDGE_MARGIN_PX.min(bounds.height() / 4.0),
    );
    Rect::from_corners(bounds.min + margin, bounds.max - margin)
}

/// Puts `node` where `target` says, returning the path it landed at.
/// `target` is read against `root` as it stands.
pub(super) fn place(
    root: &mut Block<Backend>,
    target: &Target,
    node: SceneNode<Backend>,
) -> Option<Vec<usize>> {
    match target {
        Target::Insert { parent, index } => {
            let block = block_at_mut(root, parent)?;
            let at = (*index).min(block.children.len());
            block.children.insert(at, node);

            let mut landed = parent.clone();
            landed.push(at);
            Some(landed)
        }
        Target::Merge {
            path,
            combinator,
            before,
        } => {
            let (&index, parent) = path.split_last()?;
            let block = block_at_mut(root, parent)?;
            if index >= block.children.len() {
                return None;
            }
            let host = block.children.remove(index);

            // Each delay stays on the node it belongs to; the wrapper
            // has none.
            let children = if *before {
                vec![node, host]
            } else {
                vec![host, node]
            };
            block.children.insert(
                index,
                SceneNode::Block {
                    delay: None,
                    block: Block {
                        combinator: combinator.clone(),
                        children,
                        name: None,
                    },
                },
            );

            let mut landed = parent.to_vec();
            landed.push(index);
            landed.push(if *before { 0 } else { 1 });
            Some(landed)
        }
    }
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

/// Shows the hint for `target`, or hides it when there is none.
pub(super) fn announce_hint(
    commands: &mut Commands,
    hint: &HintNode,
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
                .map(Shape::Insert)
        }
        Some(Target::Merge {
            path,
            combinator,
            before,
        }) => layout.iter().find(|placed| placed.path == *path).map(
            |placed| {
                merge_hint(rect(placed), combinator, *before, theme)
            },
        ),
        None => None,
    };

    match shown {
        Some(shape) => hint.show(commands, shape),
        None => hint.hide(commands),
    }
}

fn merge_hint(
    bounds: Rect,
    combinator: &Combinator,
    before: bool,
    theme: &EditorTheme,
) -> Shape {
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
    Shape::Merge {
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

/// The `width`-thick insert line at `index` among `parent`'s
/// children: centered in the gap between neighbours, flush against a
/// lone neighbour, or at the block's leading edge when there are
/// none.
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
        block.min.y + header_height(parent),
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

    use super::super::block_layout;
    use super::*;
    use crate::TimelineView;

    fn timed() -> SceneNode<Backend> {
        SceneNode::Draft {
            delay: None,
            duration: Duration::from_secs(1),
            name: None,
        }
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
            EditorTheme::default().space,
        );
        resolve_in(cursor, &layout, root, Some(&[1]))
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
            drop_at(&root, Vec2::new(40.0, 12.0)),
            Some(Target::Insert {
                parent: vec![0],
                index: 0
            })
        );
        assert_eq!(
            drop_at(&root, Vec2::new(280.0, 12.0)),
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
            drop_at(&root, Vec2::new(80.0, 12.0)),
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
                drop_at(&root, Vec2::new(x, 12.0)),
                merged,
                "into {inner:?}"
            );
        }
    }

    /// An `All` root of `Chain` tracks holding the given drafts.
    fn tracks(counts: &[usize]) -> Block<Backend> {
        combined(
            Combinator::All,
            counts
                .iter()
                .map(|&count| {
                    SceneNode::block(combined(
                        Combinator::Chain,
                        vec![timed(); count],
                    ))
                })
                .collect(),
        )
    }

    fn lands(root: &Block<Backend>, cursor: Vec2) -> Option<Target> {
        let layout = block_layout::layout(
            root,
            TimelineView::default(),
            &BTreeSet::new(),
            EditorTheme::default().space,
        );
        resolve(cursor, &layout, root, None)
    }

    #[test]
    fn a_drop_past_a_tracks_content_lands_at_its_end() {
        let root = tracks(&[1, 2]);

        // The second lane starts below the first's header and row.
        assert_eq!(
            lands(&root, Vec2::new(900.0, 70.0)),
            Some(Target::Insert {
                parent: vec![1],
                index: 2
            })
        );
    }

    #[test]
    fn a_drop_on_a_tracks_header_lands_in_that_track() {
        let root = tracks(&[2, 2]);

        assert_eq!(
            lands(&root, Vec2::new(50.0, 10.0)),
            Some(Target::Insert {
                parent: vec![0],
                index: 0
            })
        );
    }

    #[test]
    fn a_drop_off_the_lanes_lands_in_the_nearest_track() {
        let root = tracks(&[1, 1]);

        assert!(matches!(
            lands(&root, Vec2::new(10.0, 900.0)),
            Some(Target::Insert { parent, .. }) if parent == [1]
        ));
        assert!(matches!(
            lands(&root, Vec2::new(10.0, -20.0)),
            Some(Target::Insert { parent, .. }) if parent == [0]
        ));
    }

    #[test]
    fn nothing_lands_on_the_root_or_wraps_a_track() {
        let root = tracks(&[1, 1, 0]);
        for y in (0..200).step_by(7) {
            for x in [0.0, 30.0, 90.0, 400.0] {
                let target = lands(&root, Vec2::new(x, y as f32));
                let ok = match &target {
                    Some(Target::Insert { parent, .. }) => {
                        parent.len() == 1
                    }
                    Some(Target::Merge { path, .. }) => {
                        path.len() > 1
                    }
                    None => false,
                };
                assert!(ok, "at ({x}, {y}): {target:?}");
            }
        }
    }

    #[test]
    fn a_drop_on_a_node_in_a_track_still_wraps_it() {
        let root = tracks(&[2]);

        assert_eq!(
            lands(&root, Vec2::new(80.0, 40.0)),
            Some(Target::Merge {
                path: vec![0, 0],
                combinator: Combinator::All,
                before: false
            })
        );
    }
}
