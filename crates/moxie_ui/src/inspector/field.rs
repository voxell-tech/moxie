use std::any::TypeId;

use bevy::asset::{ReflectAsset, UntypedAssetId};
use bevy::ecs::change_detection::{ComponentTicks, Tick};
use bevy::ecs::reflect::ReflectComponent;
use bevy::prelude::*;
use bevy::reflect::{GetPath, PartialReflect};

use super::Source;

/// What a [`Field`]'s root value lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// A component of this entity.
    Entity(Entity),
    /// This asset, in its `Assets` collection.
    Asset(UntypedAssetId),
}

/// Where an inspector reads and writes: one root value, a component of
/// an entity or an asset, and the reflect path reaching a leaf inside
/// it. The empty path is the root itself.
///
/// A resource is a component too. Bevy parks each one on an entity
/// of its own, so which it was handed never comes up. That entity is
/// settled once, when the field is built.
///
/// A [`Source`] a widget can be handed. It carries no value: a widget
/// re-reads through the path whenever the root changes, so nothing
/// goes stale behind a snapshot.
///
/// Holds a [`TypeId`], so a field can be named before the world has
/// registered the type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Field {
    owner: Owner,
    root: TypeId,
    path: Box<str>,
}

impl Field {
    /// The whole of one component, which is the empty path.
    pub fn new(entity: Entity, component: TypeId) -> Self {
        Self {
            owner: Owner::Entity(entity),
            root: component,
            path: "".into(),
        }
    }

    /// The whole of one component, named by type.
    pub fn of<T: Component + Reflect>(entity: Entity) -> Self {
        Self::new(entity, TypeId::of::<T>())
    }

    /// The whole of one asset.
    pub fn asset(id: UntypedAssetId) -> Self {
        Self {
            owner: Owner::Asset(id),
            root: id.type_id(),
            path: "".into(),
        }
    }

    /// The leaf one step further in, which is how the walk descends.
    ///
    /// `name` may be a field, a tuple index, or an index into a list
    /// in brackets, as in `[2]`. An empty one is this field itself.
    pub fn child(&self, name: &str) -> Self {
        let joined = self.path.is_empty()
            || name.is_empty()
            || name.starts_with('[');
        let path = if joined {
            format!("{}{name}", self.path)
        } else {
            format!("{}.{name}", self.path)
        };

        Self {
            owner: self.owner,
            root: self.root,
            path: path.into_boxed_str(),
        }
    }

    /// The same root, back at the empty path.
    pub fn root(&self) -> Self {
        Self {
            owner: self.owner,
            root: self.root,
            path: "".into(),
        }
    }

    pub fn owner(&self) -> Owner {
        self.owner
    }

    /// The entity whose component this is, or `None` for an asset.
    pub fn entity(&self) -> Option<Entity> {
        match self.owner {
            Owner::Entity(entity) => Some(entity),
            Owner::Asset(_) => None,
        }
    }

    /// The root's type: the component's, or the asset's.
    pub fn root_type(&self) -> TypeId {
        self.root
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The type data for this field's root.
    fn type_data<D: bevy::reflect::TypeData + Clone>(
        &self,
        world: &World,
    ) -> Option<D> {
        moxie_asset::type_data::<D>(world, self.root)
    }

    /// Runs `read` against the whole root, or returns `None` when it is
    /// gone or its type was never registered with
    /// `#[reflect(Component)]` / `#[reflect(Resource)]` /
    /// `#[reflect(Asset)]`.
    pub fn read<R>(
        &self,
        world: &World,
        read: impl FnOnce(&dyn Reflect) -> R,
    ) -> Option<R> {
        let value = match self.owner {
            Owner::Entity(entity) => self
                .type_data::<ReflectComponent>(world)?
                .reflect(world.get_entity(entity).ok()?)?,
            Owner::Asset(id) => self
                .type_data::<ReflectAsset>(world)?
                .get(world, id)?,
        };
        Some(read(value))
    }

    /// As [`Self::read`], resolved to this field's own leaf rather
    /// than the component root. Misses if the path no longer resolves.
    pub fn read_at<R>(
        &self,
        world: &World,
        read: impl FnOnce(&dyn PartialReflect) -> R,
    ) -> Option<R> {
        self.read(world, |value| self.resolve(value).map(read))
            .flatten()
    }

    /// Runs `write` against the whole root.
    pub fn write<R>(
        &self,
        world: &mut World,
        write: impl FnOnce(&mut dyn Reflect) -> R,
    ) -> Option<R> {
        match self.owner {
            Owner::Entity(entity) => {
                let component =
                    self.type_data::<ReflectComponent>(world)?;
                let mut entity = world.get_entity_mut(entity).ok()?;
                let mut value = component.reflect_mut(&mut entity)?;
                Some(write(&mut *value))
            }
            Owner::Asset(id) => {
                let asset = self.type_data::<ReflectAsset>(world)?;
                Some(write(asset.get_mut(world, id)?))
            }
        }
    }

    /// Whether the root is there at all.
    pub fn exists(&self, world: &World) -> bool {
        match self.owner {
            Owner::Entity(entity) => {
                let Ok(entity) = world.get_entity(entity) else {
                    return false;
                };
                self.type_data::<ReflectComponent>(world).is_some_and(
                    |component| component.contains(entity),
                )
            }
            Owner::Asset(id) => self
                .type_data::<ReflectAsset>(world)
                .is_some_and(|asset| asset.get(world, id).is_some()),
        }
    }

    /// The leaf inside an already-read component. An empty path is
    /// the component itself, which `reflect_path` does not accept.
    fn resolve<'a>(
        &self,
        value: &'a dyn Reflect,
    ) -> Option<&'a dyn PartialReflect> {
        if self.path.is_empty() {
            Some(value.as_partial_reflect())
        } else {
            value.reflect_path(&*self.path).ok()
        }
    }

    /// The tick the root last changed on, which is what the bindings
    /// poll instead of re-reading through reflection every frame. An
    /// asset has no tick of its own, so it rides its whole `Assets`
    /// collection's.
    pub(crate) fn changed_tick(&self, world: &World) -> Option<Tick> {
        let ComponentTicks { changed, .. } = match self.owner {
            Owner::Entity(entity) => {
                let id = world.components().get_id(self.root)?;
                world
                    .get_entity(entity)
                    .ok()?
                    .get_change_ticks_by_id(id)?
            }
            Owner::Asset(_) => {
                let assets = self
                    .type_data::<ReflectAsset>(world)?
                    .assets_resource_type_id();
                let id = world.components().get_id(assets)?;
                world.get_resource_change_ticks_by_id(id)?
            }
        };
        Some(changed)
    }
}

/// Fires when the tick `read` returns differs from the last poll, and
/// on the first poll.
///
/// A write made without a system running in between gets the tick
/// already seen, so a tick that could still be written to counts as
/// changed until the next one.
pub(super) fn tick_changed(
    read: impl Fn(&World) -> Option<Tick> + Send + Sync + 'static,
) -> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen: Option<Option<Tick>> = None;
    let mut open = false;
    move |world| {
        let tick = read(world);
        let fires = open || seen != Some(tick);
        seen = Some(tick);
        open = tick == Some(world.read_change_tick());
        fires
    }
}

/// The leaf, read and written through reflection.
///
/// Change detection rides the component's tick rather than the value,
/// so polling costs a lookup instead of a reflect read every frame.
impl Source for Field {
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>> {
        self.read(world, |value| {
            Some(self.resolve(value)?.to_dynamic())
        })
        .flatten()
    }

    fn set(&self, world: &mut World, value: &dyn PartialReflect) {
        self.write(world, |component| {
            let path = &*self.path;
            let leaf = if path.is_empty() {
                Ok(component.as_partial_reflect_mut())
            } else {
                component.reflect_path_mut(path)
            };
            match leaf {
                Ok(leaf) => {
                    if let Err(err) = leaf.try_apply(value) {
                        warn!("inspector could not write {path}: {err:?}");
                    }
                }
                Err(err) => {
                    warn!("inspector lost the path {path}: {err:?}")
                }
            }
        });
    }

    fn changed(
        &self,
    ) -> Box<dyn FnMut(&World) -> bool + Send + Sync> {
        let field = self.clone();
        Box::new(tick_changed(move |world| field.changed_tick(world)))
    }

    fn boxed(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn as_field(&self) -> Option<&Field> {
        Some(self)
    }
}
