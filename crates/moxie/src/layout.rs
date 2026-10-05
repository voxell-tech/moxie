//! The dock layout as a project saves it: which windows are open,
//! how they are split, and how much room each has.

use bevy::prelude::*;
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockNode, DockSplit, DockTree, NodeId,
    SplitAxis,
};

/// The dock layout of a project, saved with it.
#[derive(Resource, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Resource, Default, Clone)]
pub struct ProjectLayout {
    /// The root first, and every node ahead of what it holds.
    pub nodes: Vec<LayoutNode>,
}

/// One area of a [`ProjectLayout`].
#[derive(Reflect, Clone, Debug, PartialEq)]
#[reflect(Clone)]
pub enum LayoutNode {
    /// Two areas side by side, or one over the other.
    Split {
        /// Whether `a` is over `b`, not left of it.
        stacked: bool,
        /// The share of the room `a` has.
        fraction: f32,
        /// Where `a` is in [`ProjectLayout::nodes`].
        a: usize,
        /// Where `b` is in [`ProjectLayout::nodes`].
        b: usize,
    },
    /// Windows as tabs, by their kind.
    Tabs {
        windows: Vec<String>,
        /// Which of `windows` is shown.
        active: usize,
    },
}

impl ProjectLayout {
    /// The layout `tree` holds.
    pub(crate) fn of(tree: &DockTree) -> Self {
        let mut nodes = Vec::new();
        if let Some(root) = tree.root {
            write(tree, root, &mut nodes);
        }
        Self { nodes }
    }

    /// The tree of this layout. `None` for an empty one, or one whose
    /// nodes do not make a tree.
    pub(crate) fn tree(&self) -> Option<DockTree> {
        let mut tree = DockTree::new();
        let root = read(&self.nodes, 0, &mut tree)?;
        tree.root = Some(root);
        Some(tree)
    }
}

/// Writes the node `id` of `tree` and all under it, and returns where
/// it went.
fn write(
    tree: &DockTree,
    id: NodeId,
    nodes: &mut Vec<LayoutNode>,
) -> usize {
    let at = nodes.len();
    match tree.get(id) {
        Some(DockNode::Split(split)) => {
            // Held ahead of its children, and filled in once they
            // have their places.
            nodes.push(LayoutNode::Tabs {
                windows: Vec::new(),
                active: 0,
            });
            let a = write(tree, split.a, nodes);
            let b = write(tree, split.b, nodes);
            nodes[at] = LayoutNode::Split {
                stacked: split.axis == SplitAxis::Vertical,
                fraction: split.fraction,
                a,
                b,
            };
        }
        Some(DockNode::Leaf(leaf)) => {
            let active = leaf
                .active
                .and_then(|active| leaf.tab_index(active))
                .unwrap_or(0);
            nodes.push(LayoutNode::Tabs {
                windows: leaf
                    .tabs()
                    .map(|(window, _)| window.to_string())
                    .collect(),
                active,
            });
        }
        None => nodes.push(LayoutNode::Tabs {
            windows: Vec::new(),
            active: 0,
        }),
    }
    at
}

/// Builds the node at `at` and all under it into `tree`. `None` when
/// a node points outside the list or back up it, which a tree never
/// does.
fn read(
    nodes: &[LayoutNode],
    at: usize,
    tree: &mut DockTree,
) -> Option<NodeId> {
    match nodes.get(at)? {
        &LayoutNode::Split {
            stacked,
            fraction,
            a,
            b,
        } => {
            if a <= at || b <= at || a == b {
                return None;
            }
            let a = read(nodes, a, tree)?;
            let b = read(nodes, b, tree)?;
            let axis = if stacked {
                SplitAxis::Vertical
            } else {
                SplitAxis::Horizontal
            };
            let split = tree.insert(DockNode::Split(DockSplit {
                axis,
                fraction,
                a,
                b,
            }));
            // Kept within what a split allows.
            tree.set_fraction(split, fraction);
            Some(split)
        }
        LayoutNode::Tabs { windows, active } => {
            let leaf = tree.insert(DockNode::Leaf(
                DockLeaf::new("", DockAreaStyle::TabBar)
                    .with_windows(windows.clone()),
            ));
            let shown = tree
                .leaf(leaf)
                .and_then(|leaf| leaf.tabs().nth(*active))
                .map(|(_, tab)| tab);
            if let Some(shown) = shown {
                tree.set_active(leaf, shown);
            }
            Some(leaf)
        }
    }
}

/// Saves the layout on screen into the [`ProjectLayout`].
pub(crate) fn capture(world: &mut World) {
    let Some(tree) = world.get_resource::<DockTree>() else {
        return;
    };
    let layout = ProjectLayout::of(tree);
    world.insert_resource(layout);
}

/// Lays the dock out as the [`ProjectLayout`] says. An empty one,
/// as a project saved without a layout has, leaves the dock alone.
pub(crate) fn apply(world: &mut World) {
    let tree = world
        .get_resource::<ProjectLayout>()
        .and_then(ProjectLayout::tree);
    if let Some(tree) = tree
        && world.contains_resource::<DockTree>()
    {
        world.insert_resource(tree);
    }
}

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::Edge;

    use super::*;

    #[test]
    fn a_layout_comes_back_as_it_was_saved() {
        let mut tree = DockTree::new();
        let viewport = tree.set_root_leaf(
            DockLeaf::new("", DockAreaStyle::TabBar)
                .with_windows(vec!["viewport".into()]),
        );
        tree.split(viewport, Edge::Bottom, "timeline".into());
        let (side, hierarchy) = tree
            .split(viewport, Edge::Right, "hierarchy".into())
            .expect("the viewport is a leaf");
        tree.add_tab(side, "assets");
        tree.set_active(side, hierarchy);
        let split = tree.parent_of(viewport).expect("just split");
        tree.set_fraction(split, 0.76);

        let layout = ProjectLayout::of(&tree);
        let back = layout.tree().expect("it is a tree");
        assert_eq!(ProjectLayout::of(&back), layout);
        assert_eq!(back.shape().is_some(), tree.shape().is_some());

        // A node that points back up the list is no tree.
        let looped = ProjectLayout {
            nodes: vec![LayoutNode::Split {
                stacked: false,
                fraction: 0.5,
                a: 0,
                b: 1,
            }],
        };
        assert!(looped.tree().is_none());
        assert!(ProjectLayout::default().tree().is_none());
    }
}
