//! Files under the project's folder, written relative to it so the
//! project can move. Anything else keeps the path it was loaded by.

use std::any::TypeId;
use std::path::Path;

use bevy::asset::{
    AssetPath, AssetServer, EphemeralHandleBehavior, HandleReference,
    HandleSerializeProcessor, LoadFromPath, ReflectHandle,
    UntypedHandle,
};
use bevy::reflect::serde::ReflectSerializerProcessor;
use bevy::reflect::{PartialReflect, TypeRegistry};
use serde::{Serialize, Serializer};

use crate::ABSOLUTE_SOURCE;

/// The source a file under the project's folder is written as. Never
/// registered: reading a project resolves it before anything loads.
const PROJECT_SOURCE: &str = "project";

/// Writes a handle to a file under `folder` as a `project://` path,
/// and any other the way bevy does.
pub(crate) struct RelativeHandles<'a> {
    pub(crate) folder: &'a Path,
}

impl ReflectSerializerProcessor for RelativeHandles<'_> {
    fn try_serialize<S: Serializer>(
        &self,
        value: &dyn PartialReflect,
        registry: &TypeRegistry,
        serializer: S,
    ) -> Result<Result<S::Ok, S>, S::Error> {
        let relative = value.try_as_reflect().and_then(|value| {
            let handle = registry
                .get(value.type_id())?
                .data::<ReflectHandle>()?
                .downcast_handle_untyped(value.as_any())?;
            relative(handle.path()?, self.folder)
        });
        match relative {
            Some(path) => {
                Ok(Ok(HandleReference::Path(path)
                    .serialize(serializer)?))
            }
            None => HandleSerializeProcessor {
                ephemeral_handle_behavior:
                    EphemeralHandleBehavior::Warn,
            }
            .try_serialize(value, registry, serializer),
        }
    }
}

/// `path` as a `project://` path, when it is a file under `folder`.
fn relative(
    path: &AssetPath<'static>,
    folder: &Path,
) -> Option<AssetPath<'static>> {
    if path.source().as_str() != Some(ABSOLUTE_SOURCE) {
        return None;
    }
    let file = Path::new("/").join(path.path());
    let within = file.strip_prefix(folder).ok()?.to_path_buf();
    let relative =
        AssetPath::from_path_buf(within).with_source(PROJECT_SOURCE);
    Some(match path.label_cow() {
        Some(label) => relative.with_label(label),
        None => relative,
    })
}

/// Loads what a project names: a `project://` path from under
/// `folder`, through the absolute source, and any other as it is.
pub(crate) struct ProjectLoader<'a> {
    pub(crate) assets: &'a mut dyn LoadFromPath,
    pub(crate) folder: &'a Path,
}

impl LoadFromPath for ProjectLoader<'_> {
    fn load_from_path_erased(
        &mut self,
        type_id: TypeId,
        path: AssetPath<'static>,
    ) -> UntypedHandle {
        let path = if path.source().as_str() == Some(PROJECT_SOURCE) {
            let file = AssetPath::from_path_buf(
                self.folder.join(path.path()),
            )
            .with_source(ABSOLUTE_SOURCE);
            match path.label_cow() {
                Some(label) => file.with_label(label),
                None => file,
            }
        } else {
            path
        };
        self.assets.load_from_path_erased(type_id, path)
    }
}

/// Loads any path, the absolute ones a project names included, which
/// the asset server alone refuses.
pub struct AnyPath<'a>(pub &'a AssetServer);

impl LoadFromPath for AnyPath<'_> {
    fn load_from_path_erased(
        &mut self,
        type_id: TypeId,
        path: AssetPath<'static>,
    ) -> UntypedHandle {
        self.0
            .load_builder()
            .override_unapproved()
            .load_erased(type_id, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Remembers the one path it was asked to load.
    #[derive(Default)]
    struct Recorded(Option<String>);

    impl LoadFromPath for Recorded {
        fn load_from_path_erased(
            &mut self,
            type_id: TypeId,
            path: AssetPath<'static>,
        ) -> UntypedHandle {
            self.0 = Some(path.to_string());
            UntypedHandle::Uuid {
                type_id,
                uuid: Default::default(),
            }
        }
    }

    fn load(path: &str, folder: &str) -> String {
        let mut recorded = Recorded::default();
        ProjectLoader {
            assets: &mut recorded,
            folder: Path::new(folder),
        }
        .load_from_path_erased(
            TypeId::of::<()>(),
            path.to_string().into(),
        );
        recorded.0.expect("it loads")
    }

    #[test]
    fn a_file_under_the_folder_is_written_relative_to_it() {
        let file = AssetPath::parse(
            "abs:///projects/intro/m/robot.glb#Mesh0",
        )
        .into_owned();
        let written = relative(&file, Path::new("/projects/intro"));
        assert_eq!(
            written.map(|path| path.to_string()),
            Some("project://m/robot.glb#Mesh0".to_string())
        );
        assert_eq!(relative(&file, Path::new("/elsewhere")), None);
    }

    #[test]
    fn a_relative_file_loads_from_where_the_project_is_now() {
        assert_eq!(
            load("project://m/robot.glb#Mesh0", "/elsewhere/intro"),
            "abs:///elsewhere/intro/m/robot.glb#Mesh0"
        );
        assert_eq!(
            load("meshes/cube.glb#Mesh0", "/elsewhere/intro"),
            "meshes/cube.glb#Mesh0"
        );
    }
}
