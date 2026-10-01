//! Inspects whatever the timeline has selected: an action's own
//! properties, or a block's.
//!
//! The reflect inspector cannot reach these: it addresses one
//! component of one entity, and an action is scene data. This edits
//! the [`EditorScene`](crate::EditorScene) directly, by the path the
//! timeline selected the node with.

// STUB: ported in wave 3 step 2. Only the panel's view is a
// placeholder; the tree walks the timeline's edits use are kept.

use bevy_fynix::{AnyView, Bevy};
use bevy_motiongfx::scene::backend::Backend;
use motiongfx_scene::block::{Block, Node};
use moxie_ui::theme::EditorTheme;

/// The action panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    // STUB: ported in wave 3 step 2
    super::stub_panel("Action")
}

/// The node `path` names.
pub(super) fn node_at<'a>(
    root: &'a Block<Backend>,
    path: &[usize],
) -> Option<&'a Node<Backend>> {
    let (&first, rest) = path.split_first()?;

    let mut node = root.children.get(first)?;
    for &index in rest {
        let Node::Block { block, .. } = node else {
            return None;
        };
        node = block.children.get(index)?;
    }
    Some(node)
}

/// The same walk, to change what it lands on.
pub(super) fn node_at_mut<'a>(
    root: &'a mut Block<Backend>,
    path: &[usize],
) -> Option<&'a mut Node<Backend>> {
    let (&first, rest) = path.split_first()?;

    let mut node = root.children.get_mut(first)?;
    for &index in rest {
        let Node::Block { block, .. } = node else {
            return None;
        };
        node = block.children.get_mut(index)?;
    }
    Some(node)
}
