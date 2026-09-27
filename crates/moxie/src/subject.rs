//! What a scene's [`SceneUid`] names in the editor's world. Every
//! other module asks here, so a new kind of subject is added in one
//! place.

use bevy::prelude::*;
use bevy_motiongfx::scene::id::{EntityUid, SceneUid, SceneUidMap};
use moxie_ui::inspector::{Field, Owner};

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

/// The subject whose value `field` reads, if it is one.
pub(crate) fn of_field(
    world: &World,
    field: &Field,
) -> Option<SceneUid> {
    match field.owner() {
        Owner::Entity(entity) => world
            .get::<EntityUid>(entity)
            .map(|&uid| SceneUid::Entity(uid)),
        Owner::Asset(_) => None,
    }
}

pub(crate) fn label(world: &World, subject: SceneUid) -> Label {
    match subject {
        SceneUid::Entity(uid) => Label {
            name: world
                .get_resource::<SceneUidMap>()
                .and_then(|map| map.entity(uid))
                .and_then(|entity| world.get::<Name>(entity))
                .map(|name| name.as_str().to_string()),
            head: uid_head(uid),
        },
    }
}

/// How much of an id a row shows.
pub(crate) fn uid_head(uid: EntityUid) -> String {
    const HEAD: usize = 8;
    uid.to_string().chars().take(HEAD).collect()
}
