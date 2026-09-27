//! The `.mox` project file: one RON document holding a project's
//! entities, as a reflected [`DynamicWorld`], its animation, its
//! bookmarks, and the [internal assets](crate::InternalAssets) it owns.
//!
//! The animation's own type is the caller's, so this reads and writes
//! any `S` serde can.

use core::marker::PhantomData;
use std::path::PathBuf;

use bevy::asset::uuid::Uuid;
use bevy::asset::{
    EphemeralHandleBehavior, HandleDeserializeProcessor,
    HandleSerializeProcessor, LoadFromPath,
};
use bevy::reflect::serde::{
    TypedReflectDeserializer, TypedReflectSerializer,
};
use bevy::reflect::{PartialReflect, TypeRegistry};
use bevy::world_serialization::DynamicWorld;
use bevy::world_serialization::serde::{
    DynamicWorldSerializer, WorldDeserializer,
};
use serde::de::{
    DeserializeOwned, DeserializeSeed, Error as _, MapAccess,
    SeqAccess, Visitor,
};
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Deserializer, Serialize, Serializer};

pub const EXTENSION: &str = "mox";

// The name a project file is written and read under, and its fields.
// Shared so the reader and the writer cannot drift apart.
const PROJECT: &str = "Project";
const WORLD: &str = "world";
const SCENE: &str = "scene";
const BOOKMARKS: &str = "bookmarks";
const ASSETS: &str = "assets";

// One internal asset's own name and fields.
const ASSET: &str = "Asset";
const TYPE: &str = "type";
const ID: &str = "id";
const NAME: &str = "name";
const VALUE: &str = "value";

/// Everything a project file holds.
pub struct ProjectFile<S> {
    pub world: DynamicWorld,
    pub scene: S,
    pub bookmarks: Vec<PathBuf>,
    pub assets: Vec<LoadedAsset>,
}

/// A project, as borrowed from wherever it lives, to be written out.
pub struct ProjectRef<'a, S> {
    pub world: &'a DynamicWorld,
    pub scene: &'a S,
    pub bookmarks: &'a [PathBuf],
    pub assets: &'a [SavedAsset<'a>],
}

/// An internal asset on its way out.
pub struct SavedAsset<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub value: &'a dyn PartialReflect,
}

/// An internal asset on its way in, as reflection reads it back.
pub struct LoadedAsset {
    pub id: Uuid,
    pub name: String,
    pub value: Box<dyn PartialReflect>,
}

/// A project as `.mox` text.
pub fn write_project<S: Serialize>(
    project: &ProjectRef<S>,
    registry: &TypeRegistry,
) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(
        &Document { project, registry },
        pretty(),
    )
}

/// A project from `.mox` text, loading the assets it names by path
/// through `assets`.
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
/// Hand-written because a [`DynamicWorld`] and an asset's reflected
/// value need the type registry to serialize at all, which no derive
/// can hand them.
struct Document<'a, S> {
    project: &'a ProjectRef<'a, S>,
    registry: &'a TypeRegistry,
}

impl<S: Serialize> Serialize for Document<'_, S> {
    fn serialize<Ser: Serializer>(
        &self,
        serializer: Ser,
    ) -> Result<Ser::Ok, Ser::Error> {
        let Self { project, registry } = *self;
        let mut out = serializer.serialize_struct(PROJECT, 4)?;
        out.serialize_field(
            WORLD,
            &DynamicWorldSerializer::new(project.world, registry),
        )?;
        out.serialize_field(SCENE, project.scene)?;
        out.serialize_field(BOOKMARKS, project.bookmarks)?;
        out.serialize_field(
            ASSETS,
            &Assets {
                assets: project.assets,
                registry,
            },
        )?;
        out.end()
    }
}

struct Assets<'a> {
    assets: &'a [SavedAsset<'a>],
    registry: &'a TypeRegistry,
}

impl Serialize for Assets<'_> {
    fn serialize<Ser: Serializer>(
        &self,
        serializer: Ser,
    ) -> Result<Ser::Ok, Ser::Error> {
        let mut seq =
            serializer.serialize_seq(Some(self.assets.len()))?;
        for asset in self.assets {
            seq.serialize_element(&Asset {
                asset,
                registry: self.registry,
            })?;
        }
        seq.end()
    }
}

struct Asset<'a> {
    asset: &'a SavedAsset<'a>,
    registry: &'a TypeRegistry,
}

impl Serialize for Asset<'_> {
    fn serialize<Ser: Serializer>(
        &self,
        serializer: Ser,
    ) -> Result<Ser::Ok, Ser::Error> {
        let Some(info) = self.asset.value.get_represented_type_info()
        else {
            return Err(serde::ser::Error::custom(
                "an internal asset's type is not known",
            ));
        };
        let processor = HandleSerializeProcessor {
            ephemeral_handle_behavior: EphemeralHandleBehavior::Warn,
        };

        let mut out = serializer.serialize_struct(ASSET, 4)?;
        // Before the value: reading it back needs the type first.
        out.serialize_field(TYPE, info.type_path())?;
        out.serialize_field(ID, &self.asset.id)?;
        out.serialize_field(NAME, self.asset.name)?;
        out.serialize_field(
            VALUE,
            &TypedReflectSerializer::with_processor(
                self.asset.value,
                self.registry,
                &processor,
            ),
        )?;
        out.end()
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
            &[WORLD, SCENE, BOOKMARKS, ASSETS],
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
        let Self {
            registry, assets, ..
        } = self;
        let mut world = None;
        let mut scene = None;
        let mut bookmarks = None;
        let mut internal = None;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                WORLD => {
                    world = Some(map.next_value_seed(
                        WorldDeserializer {
                            type_registry: registry,
                            load_from_path: &mut *assets,
                        },
                    )?);
                }
                SCENE => scene = Some(map.next_value()?),
                BOOKMARKS => bookmarks = Some(map.next_value()?),
                ASSETS => {
                    internal =
                        Some(map.next_value_seed(AssetsSeed {
                            registry,
                            assets: &mut *assets,
                        })?);
                }
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }

        Ok(ProjectFile {
            world: world
                .ok_or_else(|| A::Error::missing_field(WORLD))?,
            scene: scene
                .ok_or_else(|| A::Error::missing_field(SCENE))?,
            // Absent in a project saved before either existed.
            bookmarks: bookmarks.unwrap_or_default(),
            assets: internal.unwrap_or_default(),
        })
    }
}

struct AssetsSeed<'a> {
    registry: &'a TypeRegistry,
    assets: &'a mut dyn LoadFromPath,
}

impl<'de> DeserializeSeed<'de> for AssetsSeed<'_> {
    type Value = Vec<LoadedAsset>;

    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for AssetsSeed<'_> {
    type Value = Vec<LoadedAsset>;

    fn expecting(
        &self,
        formatter: &mut core::fmt::Formatter,
    ) -> core::fmt::Result {
        formatter.write_str("a list of internal assets")
    }

    fn visit_seq<A: SeqAccess<'de>>(
        self,
        mut seq: A,
    ) -> Result<Self::Value, A::Error> {
        let mut loaded = Vec::new();
        while let Some(asset) = seq.next_element_seed(AssetSeed {
            registry: self.registry,
            assets: &mut *self.assets,
        })? {
            loaded.push(asset);
        }
        Ok(loaded)
    }
}

struct AssetSeed<'a> {
    registry: &'a TypeRegistry,
    assets: &'a mut dyn LoadFromPath,
}

impl<'de> DeserializeSeed<'de> for AssetSeed<'_> {
    type Value = LoadedAsset;

    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(
            ASSET,
            &[TYPE, ID, NAME, VALUE],
            self,
        )
    }
}

impl<'de> Visitor<'de> for AssetSeed<'_> {
    type Value = LoadedAsset;

    fn expecting(
        &self,
        formatter: &mut core::fmt::Formatter,
    ) -> core::fmt::Result {
        formatter.write_str("an internal asset")
    }

    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> Result<Self::Value, A::Error> {
        let mut registration = None;
        let mut id = None;
        let mut name = None;
        let mut value = None;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                TYPE => {
                    let path = map.next_value::<String>()?;
                    registration = Some(
                        self.registry
                            .get_with_type_path(&path)
                            .ok_or_else(|| {
                                A::Error::custom(format!(
                                    "{path} is not a registered type"
                                ))
                            })?,
                    );
                }
                ID => id = Some(map.next_value()?),
                NAME => name = Some(map.next_value()?),
                VALUE => {
                    let registration =
                        registration.ok_or_else(|| {
                            A::Error::custom(
                                "an asset's value before its type",
                            )
                        })?;
                    let mut processor = HandleDeserializeProcessor {
                        load_from_path: &mut *self.assets,
                    };
                    value = Some(map.next_value_seed(
                        TypedReflectDeserializer::with_processor(
                            registration,
                            self.registry,
                            &mut processor,
                        ),
                    )?);
                }
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }

        Ok(LoadedAsset {
            id: id.ok_or_else(|| A::Error::missing_field(ID))?,
            name: name
                .ok_or_else(|| A::Error::missing_field(NAME))?,
            value: value
                .ok_or_else(|| A::Error::missing_field(VALUE))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use bevy::asset::{AssetPath, UntypedHandle};
    use bevy::prelude::*;

    use super::*;

    /// Nothing in these projects names an asset by path.
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

    #[derive(Reflect, Debug, PartialEq, Default)]
    struct Paint {
        red: f32,
        label: String,
    }

    #[test]
    fn round_trips() {
        let mut registry = TypeRegistry::default();
        registry.register::<Paint>();
        let bookmarks = vec![PathBuf::from("/tmp/assets")];
        let paint = Paint {
            red: 0.5,
            label: "warm".to_string(),
        };
        let id = Uuid::new_v4();

        let text = write_project(
            &ProjectRef {
                world: &DynamicWorld::default(),
                scene: &vec![1u32, 2, 3],
                bookmarks: &bookmarks,
                assets: &[SavedAsset {
                    id,
                    name: "Red",
                    value: &paint,
                }],
            },
            &registry,
        )
        .unwrap();
        let project =
            read_project::<Vec<u32>>(&text, &registry, &mut NoAssets)
                .unwrap();

        assert_eq!(project.scene, vec![1, 2, 3]);
        assert_eq!(project.bookmarks, bookmarks);
        assert!(project.world.entities.is_empty());
        let [asset] = project.assets.as_slice() else {
            panic!(
                "one asset was written, {} read",
                project.assets.len()
            );
        };
        assert_eq!((asset.id, asset.name.as_str()), (id, "Red"));
        assert!(
            asset.value.reflect_partial_eq(&paint).unwrap_or(false)
        );
    }

    #[test]
    fn missing_sections_default() {
        let text = "(world: (resources: {}, entities: {}), scene: 7)";
        let project = read_project::<u32>(
            text,
            &TypeRegistry::default(),
            &mut NoAssets,
        )
        .unwrap();

        assert_eq!(project.scene, 7);
        assert!(project.bookmarks.is_empty());
        assert!(project.assets.is_empty());
    }
}
