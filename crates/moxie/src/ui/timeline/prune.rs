//! Cleanup after a node leaves the tree: empty blocks and unused
//! stage entries.

use std::collections::HashSet;

use bevy_motiongfx::scene::asset::MotionGfxScene;
use bevy_motiongfx::scene::backend::Backend;
use bevy_motiongfx::scene::id::SceneUid;
use motiongfx_scene::block::{Block, Node as SceneNode};
use motiongfx_scene::refs::FieldRef;

/// Drops every empty block, innermost first so one emptied by losing
/// its last nested block goes too. The root stays, empty or not.
/// `keep` is a path to carry through the renumbering, cleared if it
/// pointed inside a pruned block.
pub(super) fn empty_blocks(
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
        empty_blocks(inner, &mut inner_keep);
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

/// Drops any `Stage` entry no surviving action still animates -
/// staging only ever appends when an action is created ([`create`](
/// super::create)), so deleting the last action on a field otherwise
/// leaves its seed behind forever.
pub(super) fn stage(scene: &mut MotionGfxScene) {
    let mut used = HashSet::new();
    collect_used_fields(&scene.animation, &mut used);

    for subject in &mut scene.stage.subjects {
        subject.fields.retain(|seed| {
            used.contains(&(subject.id, seed.field.clone()))
        });
    }
    scene
        .stage
        .subjects
        .retain(|subject| !subject.fields.is_empty());
}

fn collect_used_fields(
    block: &Block<Backend>,
    used: &mut HashSet<(SceneUid, FieldRef)>,
) {
    for child in &block.children {
        match child {
            SceneNode::Block { block, .. } => {
                collect_used_fields(block, used)
            }
            SceneNode::Action { action, .. } => {
                used.insert((action.subject, action.field.clone()));
            }
            SceneNode::Draft { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use bevy::asset::uuid::Uuid;
    use bevy_motiongfx::scene::backend::AnimOp;
    use bevy_motiongfx::scene::id::EntityUid;
    use bevy_motiongfx::scene::value_pool::ValuePool;
    use motiongfx_scene::block::ActionCmd;
    use motiongfx_scene::scene::{FieldSeed, Scene, Stage, Subject};

    use super::*;

    fn action(subject: SceneUid, field: &str) -> SceneNode<Backend> {
        SceneNode::action(ActionCmd {
            subject,
            field: FieldRef::new("T", field),
            op: AnimOp::To,
            value: Uuid::nil(),
            duration: Duration::ZERO,
            ease: None,
            interp: None,
            name: None,
        })
    }

    fn seeded(subject: SceneUid, field: &str) -> Subject<Backend> {
        Subject {
            id: subject,
            fields: vec![FieldSeed {
                field: FieldRef::new("T", field),
                value: Uuid::nil(),
            }],
        }
    }

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
    fn shifts_kept_past_a_removed_sibling() {
        let mut root = Block::chain(vec![block(vec![]), leaf()]);
        let mut keep = Some(vec![1]);
        empty_blocks(&mut root, &mut keep);
        assert_eq!(keep, Some(vec![0]));
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn leaves_kept_before_a_removed_sibling() {
        let mut root = Block::chain(vec![leaf(), block(vec![])]);
        let mut keep = Some(vec![0]);
        empty_blocks(&mut root, &mut keep);
        assert_eq!(keep, Some(vec![0]));
    }

    #[test]
    fn clears_kept_inside_a_removed_block() {
        let mut root =
            Block::chain(vec![block(vec![block(vec![])]), leaf()]);
        let mut keep = Some(vec![0, 0]);
        empty_blocks(&mut root, &mut keep);
        assert_eq!(keep, None);
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn rebases_kept_in_a_surviving_nested_block() {
        let mut root = Block::chain(vec![
            block(vec![block(vec![]), leaf()]),
            leaf(),
        ]);
        let mut keep = Some(vec![0, 1]);
        empty_blocks(&mut root, &mut keep);
        assert_eq!(keep, Some(vec![0, 0]));
    }

    #[test]
    fn stage_drops_a_field_no_action_targets_anymore() {
        let subject = SceneUid::Entity(EntityUid::new());
        let mut scene = MotionGfxScene(Scene {
            stage: Stage {
                subjects: vec![seeded(subject, "x")],
            },
            animation: Block::chain(vec![]),
            values: ValuePool::default(),
        });
        stage(&mut scene);
        assert!(scene.stage.subjects.is_empty());
    }

    #[test]
    fn stage_keeps_a_field_still_targeted() {
        let subject = SceneUid::Entity(EntityUid::new());
        let mut scene = MotionGfxScene(Scene {
            stage: Stage {
                subjects: vec![seeded(subject, "x")],
            },
            animation: Block::chain(vec![action(subject, "x")]),
            values: ValuePool::default(),
        });
        stage(&mut scene);
        assert_eq!(scene.stage.subjects.len(), 1);
    }

    #[test]
    fn stage_keeps_a_sibling_field_on_the_same_subject() {
        let subject = SceneUid::Entity(EntityUid::new());
        let mut fields = seeded(subject, "x").fields;
        fields.extend(seeded(subject, "y").fields);
        let mut scene = MotionGfxScene(Scene {
            stage: Stage {
                subjects: vec![Subject {
                    id: subject,
                    fields,
                }],
            },
            animation: Block::chain(vec![action(subject, "y")]),
            values: ValuePool::default(),
        });
        stage(&mut scene);
        assert_eq!(scene.stage.subjects[0].fields.len(), 1);
        assert_eq!(
            scene.stage.subjects[0].fields[0].field,
            FieldRef::new("T", "y")
        );
    }
}
