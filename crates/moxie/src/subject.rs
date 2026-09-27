//! What a scene's [`SceneUid`] names in the editor's world. Every
//! other module asks here, so a new kind of subject is added in one
//! place.

use bevy::prelude::*;
use bevy_motiongfx::scene::backend::Backend;
use bevy_motiongfx::scene::id::{EntityUid, SceneUid, SceneUidMap};
use motiongfx_scene::block::{Block, Node};
use motiongfx_scene::refs::{FieldRef, TypeName};
use moxie_ui::inspector::{Field, Owner};

use crate::EditorScene;

/// One field of one subject: what an action drives.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Target {
    pub(crate) subject: SceneUid,
    pub(crate) field: FieldRef,
}

impl Target {
    /// What an action on `field` would drive. `None` for a root, or a
    /// value that is no subject's.
    pub(crate) fn of(world: &World, field: &Field) -> Option<Self> {
        let subject = match field.owner() {
            Owner::Entity(entity) => {
                SceneUid::Entity(*world.get::<EntityUid>(entity)?)
            }
            Owner::Asset(_) => return None,
        };
        if field.path().is_empty() {
            return None;
        }
        let type_path = {
            let registry = world.resource::<AppTypeRegistry>().read();
            registry.get(field.root_type())?.type_info().type_path()
        };
        let path = format!("::{}", field.path().replace('.', "::"));
        Some(Self {
            subject,
            field: FieldRef::new(TypeName::new(type_path), path),
        })
    }

    /// Whether the scene registry knows how to drive it.
    pub(crate) fn is_animatable(&self, world: &World) -> bool {
        world.get_resource::<EditorScene>().is_some_and(|scene| {
            scene.registry().is_field_registered(&self.field)
        })
    }

    /// Whether an action anywhere in the scene already drives it.
    pub(crate) fn has_action(&self, world: &World) -> bool {
        world.get_resource::<EditorScene>().is_some_and(|scene| {
            self.driven_in(&scene.scene().0.animation)
        })
    }

    fn driven_in(&self, block: &Block<Backend>) -> bool {
        block.children.iter().any(|child| match child {
            Node::Block { block, .. } => self.driven_in(block),
            Node::Action { action, .. } => {
                action.subject == self.subject
                    && action.field == self.field
            }
            Node::Draft { .. } => false,
        })
    }
}

/// A subject as a row shows it, split so its id can render muted where
/// its name cannot.
#[derive(Clone, PartialEq)]
pub(crate) struct Label {
    /// Its name, while it is still in the world.
    pub(crate) name: Option<String>,
    /// The head of its id: a whole uuid is unreadable, but two subjects
    /// sharing a name still need telling apart.
    pub(crate) head: String,
}

impl Label {
    pub(crate) fn of(world: &World, subject: SceneUid) -> Self {
        match subject {
            SceneUid::Entity(uid) => Self {
                name: world
                    .get_resource::<SceneUidMap>()
                    .and_then(|map| map.entity(uid))
                    .and_then(|entity| world.get::<Name>(entity))
                    .map(|name| name.as_str().to_string()),
                head: uid_head(uid),
            },
        }
    }
}

/// How much of an id a row shows.
pub(crate) fn uid_head(uid: EntityUid) -> String {
    const HEAD: usize = 8;
    uid.to_string().chars().take(HEAD).collect()
}
