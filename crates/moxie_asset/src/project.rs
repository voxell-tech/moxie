//! The `.mox` project file: one RON document holding a project's
//! entities, as a reflected [`DynamicWorld`], its animation, and its
//! bookmarks.
//!
//! The animation's own type is the caller's, so this reads and writes
//! any `S` serde can.

use core::marker::PhantomData;
use std::path::PathBuf;

use bevy::asset::LoadFromPath;
use bevy::reflect::TypeRegistry;
use bevy::world_serialization::DynamicWorld;
use bevy::world_serialization::serde::{
    DynamicWorldSerializer, WorldDeserializer,
};
use serde::de::{
    DeserializeOwned, DeserializeSeed, MapAccess, Visitor,
};
use serde::ser::SerializeStruct;
use serde::{Deserializer, Serialize, Serializer};

pub const EXTENSION: &str = "mox";

// The name a project file is written and read under, and its fields.
// Shared so the reader and the writer cannot drift apart.
const PROJECT: &str = "Project";
const WORLD: &str = "world";
const SCENE: &str = "scene";
const BOOKMARKS: &str = "bookmarks";

/// Everything a project file holds.
pub struct ProjectFile<S> {
    pub world: DynamicWorld,
    pub scene: S,
    pub bookmarks: Vec<PathBuf>,
}

/// A project as `.mox` text.
pub fn write_project<S: Serialize>(
    world: &DynamicWorld,
    scene: &S,
    bookmarks: &[PathBuf],
    registry: &TypeRegistry,
) -> Result<String, ron::Error> {
    let document = Document {
        world,
        scene,
        bookmarks,
        registry,
    };
    ron::ser::to_string_pretty(&document, pretty())
}

/// A project from `.mox` text, loading the assets its components
/// name through `assets`.
pub fn read_project<S: DeserializeOwned>(
    text: &str,
    registry: &TypeRegistry,
    assets: &mut dyn LoadFromPath,
) -> Result<ProjectFile<S>, ron::de::SpannedError> {
    ron::Options::default().from_str_seed(
        text,
        ProjectSeed {
            registry,
            assets,
            scene: PhantomData,
        },
    )
}

fn pretty() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::default()
        .indentor("  ".to_string())
        .new_line("\n".to_string())
}

/// The project, on its way out.
///
/// Hand-written because a [`DynamicWorld`] needs the type registry to
/// serialize at all, which no derive can hand it.
struct Document<'a, S> {
    world: &'a DynamicWorld,
    scene: &'a S,
    bookmarks: &'a [PathBuf],
    registry: &'a TypeRegistry,
}

impl<S: Serialize> Serialize for Document<'_, S> {
    fn serialize<Ser: Serializer>(
        &self,
        serializer: Ser,
    ) -> Result<Ser::Ok, Ser::Error> {
        let mut project = serializer.serialize_struct(PROJECT, 3)?;
        project.serialize_field(
            WORLD,
            &DynamicWorldSerializer::new(self.world, self.registry),
        )?;
        project.serialize_field(SCENE, self.scene)?;
        project.serialize_field(BOOKMARKS, self.bookmarks)?;
        project.end()
    }
}

/// The project, on its way in. Carries what the world half needs: the
/// registry to read components through, and somewhere for the asset
/// paths in them to be loaded from.
struct ProjectSeed<'a, S> {
    registry: &'a TypeRegistry,
    assets: &'a mut dyn LoadFromPath,
    scene: PhantomData<fn() -> S>,
}

impl<'de, S: DeserializeOwned> DeserializeSeed<'de>
    for ProjectSeed<'_, S>
{
    type Value = ProjectFile<S>;

    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(
            PROJECT,
            &[WORLD, SCENE, BOOKMARKS],
            self,
        )
    }
}

impl<'de, S: DeserializeOwned> Visitor<'de> for ProjectSeed<'_, S> {
    type Value = ProjectFile<S>;

    fn expecting(
        &self,
        formatter: &mut core::fmt::Formatter,
    ) -> core::fmt::Result {
        formatter.write_str("a moxie project")
    }

    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> Result<Self::Value, A::Error> {
        let mut world = None;
        let mut scene = None;
        let mut bookmarks = None;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                WORLD => {
                    world = Some(map.next_value_seed(
                        WorldDeserializer {
                            type_registry: self.registry,
                            load_from_path: self.assets,
                        },
                    )?);
                }
                SCENE => scene = Some(map.next_value()?),
                BOOKMARKS => bookmarks = Some(map.next_value()?),
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }

        Ok(ProjectFile {
            world: world.ok_or_else(|| {
                serde::de::Error::missing_field(WORLD)
            })?,
            scene: scene.ok_or_else(|| {
                serde::de::Error::missing_field(SCENE)
            })?,
            // Absent in a project saved before bookmarks existed.
            bookmarks: bookmarks.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use bevy::asset::{AssetPath, UntypedHandle};

    use super::*;

    /// Nothing in these projects names an asset.
    struct NoAssets;

    impl LoadFromPath for NoAssets {
        fn load_from_path_erased(
            &mut self,
            _: TypeId,
            path: AssetPath<'static>,
        ) -> UntypedHandle {
            unreachable!(
                "nothing here names an asset, yet {path} was"
            )
        }
    }

    #[test]
    fn round_trips() {
        let registry = TypeRegistry::default();
        let bookmarks = vec![PathBuf::from("/tmp/assets")];

        let text = write_project(
            &DynamicWorld::default(),
            &vec![1u32, 2, 3],
            &bookmarks,
            &registry,
        )
        .unwrap();
        let project =
            read_project::<Vec<u32>>(&text, &registry, &mut NoAssets)
                .unwrap();

        assert_eq!(project.scene, vec![1, 2, 3]);
        assert_eq!(project.bookmarks, bookmarks);
        assert!(project.world.entities.is_empty());
    }

    #[test]
    fn bookmarks_default_when_absent() {
        let text = "(world: (resources: {}, entities: {}), scene: 7)";
        let project = read_project::<u32>(
            text,
            &TypeRegistry::default(),
            &mut NoAssets,
        )
        .unwrap();

        assert_eq!(project.scene, 7);
        assert!(project.bookmarks.is_empty());
    }
}
