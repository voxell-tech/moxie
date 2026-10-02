//! Reflection-driven inspector.
//!
//! [`inspector_fields`] walks any reflected value in the world and
//! renders it as a collapsible hierarchy of editable rows. Which
//! editor a leaf gets is a type-registry lookup, so a new editable
//! type is one [`Inspect`] impl away.
//!
//! An editor is handed a [`Binding`] rather than a value, and never
//! learns where that value actually lives. [`Field`] (a component of
//! an entity) is the one the walk uses, but anything else the editor
//! keeps can serve the same editors through a [`Source`].

mod enums;
mod field;
mod field_drag;
mod handle;
mod primitive;
mod text;
mod tree;
mod vector;

use core::time::Duration;
use std::any::TypeId;

use bevy::light::CascadeShadowConfig;
use bevy::prelude::*;
use bevy::reflect::std_traits::ReflectDefault;
use bevy::reflect::{FromType, GetTypeRegistration, PartialReflect};
use bevy::sprite::Anchor;
use bevy::text::{LetterSpacing, LineHeight};
use bevy_fynix::views::{FrameProps as _, column, row};
use bevy_fynix::{AnyView, Bevy, Signal, View, ViewExt as _};
pub use enums::variant_picker;
pub use field::{Field, Owner};
pub use field_drag::{
    DraggableField, DraggedField, FieldAnimatable, FieldHasAction,
    FieldName, draggable_field, field_name,
};
use moxie_asset::type_data;
pub use tree::{inspector_fields, section, variant_fields};
pub(crate) use tree::{root_leaf, section_open, toggle_section};

use crate::fold;
use crate::theme::EditorTheme;

/// An editor, as the registry stores it: a function from where the
/// value lives to the view that edits it.
pub type Editor = fn(Binding) -> AnyView<Bevy, EditorTheme>;

/// The editors and the entity-inspector sections available out of
/// the box.
///
/// Anything else is one [`InspectAppExt::register_inspect`] or
/// [`InspectAppExt::register_inspectable`] away, and needs no change
/// here.
pub struct InspectPlugin;

impl Plugin for InspectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FieldAnimatable>()
            .init_resource::<FieldHasAction>()
            .init_resource::<DraggedField>();

        app.register_inspect::<bool>()
            .register_inspect::<f32>()
            .register_inspect::<f64>()
            .register_inspect::<i32>()
            .register_inspect::<i64>()
            .register_inspect::<u32>()
            .register_inspect::<u64>()
            .register_inspect::<Duration>()
            .register_inspect::<Vec2>()
            .register_inspect::<Vec3>()
            .register_inspect::<Vec4>()
            .register_inspect::<IVec2>()
            .register_inspect::<IVec3>()
            .register_inspect::<IVec4>()
            .register_inspect::<UVec2>()
            .register_inspect::<UVec3>()
            .register_inspect::<UVec4>()
            .register_inspect::<Quat>()
            .register_inspect::<String>()
            .register_inspect::<Name>()
            .register_inspect::<Handle<StandardMaterial>>()
            .register_inspect::<Handle<Mesh>>()
            .register_inspect::<Handle<ColorMaterial>>()
            .register_inspect::<Handle<Font>>();

        app.register_inspectable::<Name>()
            .register_inspectable::<Visibility>()
            .register_inspectable::<Transform>();

        app.register_essential_with::<Name>(|| {
            Box::new(Name::new("New Entity"))
        })
        .register_essential::<Visibility>()
        .register_essential::<Transform>();

        app.with_inspect_group("Text")
            .register_inspectable::<Text2d>()
            .register_inspectable::<TextFont>()
            .register_inspectable::<TextColor>()
            .register_inspectable::<TextLayout>()
            .register_inspectable::<LineHeight>()
            .register_inspectable::<LetterSpacing>()
            .register_inspectable::<Anchor>();

        app.with_inspect_group("2D Mesh")
            .register_inspectable::<Mesh2d>()
            .register_inspectable_as::<MeshMaterial2d<ColorMaterial>>(
                "Color Material",
            );

        app.with_inspect_group("Cameras")
            .register_inspectable::<Camera3d>()
            .register_inspectable::<Camera2d>();

        app.with_inspect_group("Lighting")
            .register_inspectable::<CascadeShadowConfig>()
            .register_inspectable::<DirectionalLight>()
            .register_inspectable::<PointLight>()
            .register_inspectable::<RectLight>()
            .register_inspectable::<SpotLight>();

        app.with_inspect_group("3D Mesh")
            .register_inspectable::<Mesh3d>()
            .register_inspectable_as::<MeshMaterial3d<StandardMaterial>>(
                "PBR Material",
            );
    }
}

/// Registering inspector editors on the app.
pub trait InspectAppExt {
    /// Makes `T` editable wherever the inspector meets it.
    fn register_inspect<T: Inspect>(&mut self) -> &mut Self;

    /// Makes `T` a section of its own wherever an
    /// [`entity_inspector`](crate::elements::entity_inspector) meets
    /// it.
    ///
    /// Opt-in like [`register_inspect`](Self::register_inspect):
    /// `#[reflect(Component)]` lets the inspector reach a value, not
    /// decide it's worth a row. Bevy reflects plenty nobody authors,
    /// like `GlobalTransform`.
    fn register_inspectable<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
    ) -> &mut Self;

    /// As [`register_inspectable`](Self::register_inspectable), with
    /// `name` heading the section instead of `T`'s own name split
    /// into words.
    fn register_inspectable_as<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
        name: &'static str,
    ) -> &mut Self;

    /// A scope for tagging the [`InspectGroup::register_inspectable`]
    /// calls chained off it with `name`, so the add-component menu
    /// lists them together.
    fn with_inspect_group(
        &mut self,
        name: &'static str,
    ) -> InspectGroup<'_>;

    /// Marks `T` a component no fresh entity is ever without: also
    /// registers `T`'s [`ReflectDefault`], what actually spawns it on
    /// one, and [`entity_inspector`](
    /// crate::elements::entity_inspector) never offers to delete it.
    fn register_essential<
        T: Component
            + Reflect
            + TypePath
            + GetTypeRegistration
            + Default,
    >(
        &mut self,
    ) -> &mut Self;

    /// As [`register_essential`](Self::register_essential), spawning
    /// `T` with `spawn` instead of [`Default::default`] - a
    /// reasonable non-empty [`Name`] for a fresh entity, say, rather
    /// than an empty string.
    fn register_essential_with<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
        spawn: fn() -> Box<dyn Reflect>,
    ) -> &mut Self;
}

impl InspectAppExt for App {
    fn register_inspect<T: Inspect>(&mut self) -> &mut Self {
        self.register_type::<T>()
            .register_type_data::<T, ReflectInspect>()
    }

    fn register_inspectable<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
    ) -> &mut Self {
        self.register_type::<T>()
            .register_type_data::<T, ReflectInspectable>()
    }

    fn register_inspectable_as<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
        name: &'static str,
    ) -> &mut Self {
        self.register_type::<T>();
        let registry =
            self.world().resource::<AppTypeRegistry>().clone();
        let mut registry = registry.write();
        if let Some(registration) =
            registry.get_mut(TypeId::of::<T>())
        {
            registration
                .insert(ReflectInspectable { name: Some(name) });
        }
        self
    }

    fn with_inspect_group(
        &mut self,
        name: &'static str,
    ) -> InspectGroup<'_> {
        InspectGroup { app: self, name }
    }

    fn register_essential<
        T: Component
            + Reflect
            + TypePath
            + GetTypeRegistration
            + Default,
    >(
        &mut self,
    ) -> &mut Self {
        self.register_type::<T>()
            .register_type_data::<T, ReflectDefault>()
            .register_type_data::<T, ReflectEssential>()
    }

    fn register_essential_with<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
        spawn: fn() -> Box<dyn Reflect>,
    ) -> &mut Self {
        self.register_type::<T>();
        let registry =
            self.world().resource::<AppTypeRegistry>().clone();
        let mut registry = registry.write();
        if let Some(registration) =
            registry.get_mut(TypeId::of::<T>())
        {
            registration.insert(ReflectEssential { spawn });
        }
        self
    }
}

/// A component's group in the add-component menu; see
/// [`InspectAppExt::with_inspect_group`].
#[derive(Clone)]
pub struct ReflectInspectGroup(pub &'static str);

/// Marks a component no fresh entity is ever without; see
/// [`InspectAppExt::register_essential`] and
/// [`InspectAppExt::register_essential_with`].
#[derive(Clone, Copy)]
pub struct ReflectEssential {
    // A bare fn: like `ReflectDefault`, the value it produces
    // carries all the state it needs, so there is nothing for the
    // function itself to capture.
    spawn: fn() -> Box<dyn Reflect>,
}

impl ReflectEssential {
    /// The value a fresh entity gets for this component.
    pub fn spawn(&self) -> Box<dyn Reflect> {
        (self.spawn)()
    }
}

impl<T: Component + Reflect + Default> FromType<T>
    for ReflectEssential
{
    fn from_type() -> Self {
        Self {
            spawn: || Box::new(T::default()),
        }
    }
}

/// A scope from [`InspectAppExt::with_inspect_group`], for tagging
/// several [`register_inspectable`](Self::register_inspectable) calls
/// at once.
pub struct InspectGroup<'a> {
    app: &'a mut App,
    name: &'static str,
}

impl InspectGroup<'_> {
    /// As [`InspectAppExt::register_inspectable`], tagged with this
    /// scope's group.
    pub fn register_inspectable<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
    ) -> &mut Self {
        self.app.register_inspectable::<T>();
        self.tag::<T>();
        self
    }

    /// As [`InspectAppExt::register_inspectable_as`], tagged with
    /// this scope's group.
    pub fn register_inspectable_as<
        T: Component + Reflect + TypePath + GetTypeRegistration,
    >(
        &mut self,
        name: &'static str,
    ) -> &mut Self {
        self.app.register_inspectable_as::<T>(name);
        self.tag::<T>();
        self
    }

    /// Inserts [`ReflectInspectGroup`] onto `T`'s already-registered
    /// entry.
    fn tag<T: TypePath + GetTypeRegistration>(&mut self) {
        let registry =
            self.app.world().resource::<AppTypeRegistry>().clone();
        let mut registry = registry.write();
        if let Some(registration) =
            registry.get_mut(TypeId::of::<T>())
        {
            registration.insert(ReflectInspectGroup(self.name));
        }
    }
}

/// The place an editor reads and writes the value it edits.
///
/// Reflected rather than typed, so it can be handed to whichever
/// editor the registry picked for an unknown type.
pub trait Source: Send + Sync + 'static {
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>>;

    fn set(&self, world: &mut World, value: &dyn PartialReflect);

    /// Fires when the value may have moved, and on the first poll.
    /// Each source picks its own cheapest signal.
    fn changed(&self)
    -> Box<dyn FnMut(&World) -> bool + Send + Sync>;

    /// A copy of its own, for an editor that needs one per input.
    fn boxed(&self) -> Box<dyn Source>;

    /// The component field this reads and writes, when it is one.
    /// `None` for a source backed by something the editor keeps
    /// elsewhere.
    fn as_field(&self) -> Option<&Field> {
        None
    }
}

/// Reading and writing a source as a concrete type, which is what an
/// editor actually wants.
pub trait SourceExt: Source {
    fn read<T: FromReflect>(&self, world: &World) -> Option<T> {
        T::from_reflect(&*self.get(world)?)
    }

    /// Skips the write if `value` is what the source already holds.
    /// A field commits on blur as well as on edit, which would
    /// otherwise bump the component's tick for nothing.
    fn write<T: PartialReflect>(&self, world: &mut World, value: T) {
        let unchanged = self.get(world).is_some_and(|current| {
            current.reflect_partial_eq(&value).unwrap_or(false)
        });
        if !unchanged {
            self.set(world, &value);
        }
    }
}

impl<S: Source + ?Sized> SourceExt for S {}

/// A [`Source`] an editor is handed, cloneable so each input of an
/// editor can hold its own.
///
/// It carries no value: an editor binds a [`signal`](Self::signal) of
/// it to its inputs and writes back through [`write`](Self::write),
/// so nothing goes stale behind a snapshot.
pub struct Binding(Box<dyn Source>);

impl Clone for Binding {
    fn clone(&self) -> Self {
        Self(self.0.boxed())
    }
}

impl From<Field> for Binding {
    fn from(field: Field) -> Self {
        Self::new(field)
    }
}

impl Binding {
    /// A binding to what `source` reads and writes.
    pub fn new(source: impl Source) -> Self {
        Self(Box::new(source))
    }

    /// The component field this reads and writes, when it is one.
    pub fn field(&self) -> Option<&Field> {
        self.0.as_field()
    }

    /// The value now, as a reflected copy.
    pub fn get(
        &self,
        world: &World,
    ) -> Option<Box<dyn PartialReflect>> {
        self.0.get(world)
    }

    /// The value now, as `T`.
    pub fn read<T: FromReflect>(&self, world: &World) -> Option<T> {
        self.0.read(world)
    }

    /// Writes `value`, unless the source already holds it.
    pub fn write<T: PartialReflect>(
        &self,
        world: &mut World,
        value: T,
    ) {
        self.0.write(world, value);
    }

    /// Writes a reflected `value`.
    pub fn set(&self, world: &mut World, value: &dyn PartialReflect) {
        self.0.set(world, value);
    }

    /// A signal of `read` run against this binding, re-read when the
    /// source says the value may have moved.
    pub fn derive<T>(
        &self,
        read: impl Fn(&Binding, &World) -> T + Send + Sync + 'static,
    ) -> Signal<T> {
        let binding = self.clone();
        Signal::new(
            move |world: &World| read(&binding, world),
            self.0.changed(),
        )
    }

    /// A signal of the value as `T`, `T::default()` while it cannot
    /// be read.
    pub fn signal<T: FromReflect + Default>(&self) -> Signal<T> {
        self.derive(|binding, world| {
            binding.read::<T>(world).unwrap_or_default()
        })
    }
}

/// Fires when `get`'s value differs from the last poll, and on the
/// first poll. For a [`Source::changed`] with no tick to ride:
/// `PartialReflect` has no `PartialEq`, so this compares through
/// [`PartialReflect::reflect_partial_eq`] instead.
pub fn reflect_changed(
    get: impl Fn(&World) -> Option<Box<dyn PartialReflect>>
    + Send
    + Sync
    + 'static,
) -> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen: Option<Option<Box<dyn PartialReflect>>> = None;
    move |world| {
        let current = get(world);
        let fired = !seen.as_ref().is_some_and(|last| {
            match (last, &current) {
                (Some(last), Some(current)) => last
                    .reflect_partial_eq(&**current)
                    .unwrap_or(false),
                (None, None) => true,
                _ => false,
            }
        });
        seen = Some(current);
        fired
    }
}

/// The editor for whatever `binding` holds when this is built.
///
/// A registered [`Inspect`] wins; failing that an enum picks its own
/// variant, which needs no registration because reflection already
/// knows what the variants are. Anything else is an empty node.
pub fn inspect_value(binding: Binding) -> AnyView<Bevy, EditorTheme> {
    AnyView::new(move |cx| {
        let value = binding.get(cx.world);
        let drawer = value.as_deref().and_then(|value| {
            let type_id =
                value.get_represented_type_info()?.type_id();
            type_data::<ReflectInspect>(cx.world, type_id)
        });
        let view = match drawer {
            Some(drawer) => drawer.build(binding),
            None => variant_picker(binding),
        };
        cx.build(view)
    })
}

/// The label column's share of the row.
const LABEL_SHARE: f32 = 0.4;

/// The gap between a row's label and its value.
const LABEL_GAP: f32 = 8.0;

/// One field's row: a label column, then `value` beside it, split
/// 40/60 so it scales with the panel's width.
///
/// `depth` is how many [`Foldable`](crate::fold::Foldable) bodies
/// this row sits under. The label sheds their indent, so `value`
/// starts at the same place however deep the row is nested.
///
/// A row with no `label` has no label column, and `value` takes the
/// whole row.
pub fn field_row(
    label: Option<AnyView<Bevy, EditorTheme>>,
    value: AnyView<Bevy, EditorTheme>,
    depth: u32,
) -> impl View<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let space = cx.theme().space;
        let indent = space.fold_toggle / 2.0
            + fold::RAIL_WIDTH
            + space.fold_indent;
        let shed = (1.0 - LABEL_SHARE) * depth as f32 * indent;

        let value = column((value,)).grow(1.0).gap(0.0);
        let mut columns = Vec::new();
        if let Some(label) = label {
            columns.push(
                column((label,))
                    .width(percent(LABEL_SHARE * 100.0))
                    .margin(UiRect::right(px(-shed)))
                    .overflow(Overflow::clip_x())
                    .padding(UiRect::right(px(LABEL_GAP)))
                    .gap(0.0)
                    .boxed(),
            );
        }
        columns.push(value.boxed());
        cx.build(
            row(columns)
                .width(percent(100.0))
                .align(AlignItems::Center)
                .gap(LABEL_GAP)
                .padding(UiRect::vertical(px(3.0))),
        )
    })
}

/// Builds the editor for one reflected value.
///
/// The value itself is not passed in. An editor is built once, then
/// its inputs bind to a [`Binding::signal`] and re-read whenever that
/// fires, so a focused input survives an edit.
pub trait Inspect:
    FromReflect + TypePath + GetTypeRegistration
{
    fn build(binding: Binding) -> AnyView<Bevy, EditorTheme>;
}

/// Type data pointing at a type's [`Inspect::build`].
///
/// A bare `fn`: the binding arrives as an argument, so nothing about
/// the editor has to be boxed to be stored.
#[derive(Clone)]
pub struct ReflectInspect {
    build: Editor,
}

impl ReflectInspect {
    pub fn build(
        &self,
        binding: Binding,
    ) -> AnyView<Bevy, EditorTheme> {
        (self.build)(binding)
    }
}

impl<T: Inspect> FromType<T> for ReflectInspect {
    fn from_type() -> Self {
        Self { build: T::build }
    }
}

/// Marks a component as one an [`entity_inspector`](
/// crate::elements::entity_inspector) shows. See
/// [`InspectAppExt::register_inspectable`].
#[derive(Clone)]
pub struct ReflectInspectable {
    /// What the section is headed with instead of the type's own
    /// name, split into words. See
    /// [`InspectAppExt::register_inspectable_as`].
    pub name: Option<&'static str>,
}

impl<T: Component + Reflect> FromType<T> for ReflectInspectable {
    fn from_type() -> Self {
        Self { name: None }
    }
}
