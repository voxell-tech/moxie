//! Reflection-driven inspector.
//!
//! [`InspectorFields`] walks any reflected value in the world and
//! renders it as a collapsible hierarchy of editable rows. Which
//! widget a leaf gets is a type-registry lookup, so a new editable
//! type is one [`Inspect`] impl away.
//!
//! A widget is handed a [`Source`] rather than a value, and never
//! learns where that value actually lives. [`Field`] (a component of
//! an entity) is the one the walk uses, but anything else the editor
//! keeps can serve the same widgets.

mod enums;
mod field;
mod field_drag;
mod handle;
mod primitive;
mod text;
mod tree;
mod vector;

use std::any::TypeId;

use bevy::light::CascadeShadowConfig;
use bevy::prelude::*;
use bevy::reflect::std_traits::ReflectDefault;
use bevy::reflect::{FromType, GetTypeRegistration, PartialReflect};
use bevy::sprite::Anchor;
use bevy::text::{LetterSpacing, LineHeight};
use fynix::composer::Composer;
use fynix::prelude::*;
use moxie_asset::AssetKindAppExt as _;

use crate::elements::{Frame, Label};
use crate::fold;
use crate::reactive::{BevyUi, FynixHost};

pub use field::Field;
use field_drag::FieldName;
pub(crate) use field_drag::draggable_field;
pub use field_drag::{DraggedField, FieldAnimatable, FieldHasAction};
pub use tree::{InspectorFields, Section};
pub(crate) use tree::{root_leaf, section_open, toggle_section};

/// The widgets and the entity-inspector sections available out of
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

        app.register_asset_kind::<StandardMaterial>(&["mat"]);
    }
}

/// Registering inspector widgets on the app.
pub trait InspectAppExt {
    /// Makes `T` editable wherever the inspector meets it.
    fn register_inspect<T: Inspect>(&mut self) -> &mut Self;

    /// Makes `T` a section of its own wherever an [`EntityInspector`](
    /// crate::elements::EntityInspector) meets it.
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
    /// calls chained off it with `name`, so `AddComponent`'s menu
    /// lists them together.
    fn with_inspect_group(
        &mut self,
        name: &'static str,
    ) -> InspectGroup<'_>;

    /// Marks `T` a component no fresh entity is ever without: also
    /// registers `T`'s [`ReflectDefault`], what actually spawns it on
    /// one, and [`EntityInspector`](crate::elements::EntityInspector)
    /// never offers to delete it.
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

/// Which group of `AddComponent`'s menu a component belongs to; see
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

/// Where a widget reads and writes the value it edits.
///
/// Reflected rather than typed, so it can be handed to whichever
/// widget the registry picked for an unknown type.
pub trait Source: Send + Sync + 'static {
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>>;

    fn set(&self, world: &mut World, value: &dyn PartialReflect);

    /// Fires when the value may have moved, and on the first poll.
    /// Each source picks its own cheapest signal.
    fn changed(&self)
    -> Box<dyn FnMut(&World) -> bool + Send + Sync>;

    /// A copy of its own, for a widget that needs one per input.
    fn boxed(&self) -> Box<dyn Source>;

    /// The component field this reads and writes, when it is one.
    /// `None` for a source backed by something the editor keeps
    /// elsewhere.
    fn as_field(&self) -> Option<&Field> {
        None
    }
}

/// Reading and writing a source as a concrete type, which is what a
/// widget actually wants.
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

/// A source's signal, in the shape the kernel polls with. Nothing
/// about a source depends on the node asking.
pub fn when_changed(
    source: &dyn Source,
) -> impl for<'w> FnMut(WorldNodeRef<'w, FynixHost>) -> bool
+ Send
+ Sync
+ 'static {
    let mut changed = source.changed();
    move |WorldNodeRef { world, .. }| changed(world)
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

/// The widget for whatever `source` currently holds.
///
/// A registered [`Inspect`] wins; failing that an enum picks its own
/// variant, which needs no registration because reflection already
/// knows what the variants are.
pub fn inspect_value(ui: &mut BevyUi, source: &dyn Source) {
    let Some(value) = source.get(ui.world) else {
        return;
    };

    let drawer = value
        .get_represented_type_info()
        .map(|info| info.type_id())
        .and_then(|type_id| {
            let registry =
                ui.world.resource::<AppTypeRegistry>().read();
            registry.get_type_data::<ReflectInspect>(type_id).cloned()
        });

    if let Some(drawer) = drawer {
        drawer.build(source, ui);
    } else if let Some(variants) = enums::variants(&*value) {
        let pick = {
            let registry =
                ui.world.resource::<AppTypeRegistry>().read();
            enums::constructible(&*value, &registry)
        };
        ui.compose(enums::VariantPicker {
            source,
            variants,
            pick,
        });
    }
}

/// One field's row: a label column, then whatever `value` builds
/// beside it. The split is proportional (40/60), so it scales with
/// however wide the panel is docked - the same convention Unity,
/// Godot, and Unreal's own inspectors use.
///
/// `depth` is how many [`Foldable`](crate::fold::Foldable) bodies
/// this row sits under. Each one narrows the row by its own indent,
/// which would otherwise pull the 40% mark inward with it; the label
/// sheds that same width back so `value` starts at the same place
/// no matter how deep its row is nested.
pub struct FieldRow<F: FnOnce(&mut BevyUi)> {
    pub label: String,
    pub color: Color,
    pub bold: bool,
    pub depth: u32,
    pub value: F,
    /// The component field this row edits, when it is one. An
    /// animatable one (per [`FieldAnimatable`]) gets a draggable label;
    /// `None` never does.
    pub field: Option<Field>,
}

impl<F: FnOnce(&mut BevyUi)> Composer<FynixHost> for FieldRow<F> {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let Self {
            label,
            color,
            bold,
            depth,
            value,
            field,
        } = self;

        const VALUE_SHARE: f32 = 0.6;
        const LABEL_SIZE: f32 = 12.0;
        const EDGE_PADDING: f32 = 8.0;
        let indent = ui.theme.space.fold_indent + fold::RAIL_WIDTH;
        let shed = VALUE_SHARE * depth as f32 * indent;

        ui.elem(elem!(
            Frame,
            width = percent(100),
            direction = FlexDirection::Row,
            align = AlignItems::Center,
            column_gap = px(8),
            padding = UiRect::vertical(px(3))
        ))
        .with(move |ui| {
            // A spliced field has no name of its own - see `entries`
            // in `tree.rs` - so there is nothing to head a label
            // column with; the value takes the whole row instead.
            if !label.is_empty() {
                ui.elem(elem!(
                    Frame,
                    width = percent(40),
                    margin = UiRect::right(px(-shed)),
                    overflow = Overflow::clip_x(),
                    padding = UiRect::right(px(EDGE_PADDING))
                ))
                .with(move |ui| match field {
                    Some(field) => {
                        ui.compose(FieldName {
                            field,
                            text: label,
                            size: LABEL_SIZE,
                            color,
                            bold,
                        });
                    }
                    None => {
                        ui.elem(elem!(
                            Label,
                            text = label,
                            size = LABEL_SIZE,
                            color = color,
                            bold = bold,
                            wrap = false
                        ));
                    }
                });
            }
            ui.elem(elem!(Frame, flex_grow = 1.0f32)).with(value);
        })
        .handle()
    }
}

/// Builds the editing widget for one reflected value.
///
/// The value itself is not passed in. A widget is built once, then
/// binds to its source and re-reads whenever that fires, so a
/// focused input survives an edit.
pub trait Inspect:
    FromReflect + TypePath + GetTypeRegistration
{
    fn build(source: &dyn Source, ui: &mut BevyUi);
}

/// Type data pointing at a type's [`Inspect::build`].
///
/// A bare `fn`: the source arrives as an argument, so nothing about
/// the widget has to be boxed to be stored.
#[derive(Clone)]
pub struct ReflectInspect {
    build: fn(&dyn Source, &mut BevyUi),
}

impl ReflectInspect {
    pub fn build(&self, source: &dyn Source, ui: &mut BevyUi) {
        (self.build)(source, ui)
    }
}

impl<T: Inspect> FromType<T> for ReflectInspect {
    fn from_type() -> Self {
        Self { build: T::build }
    }
}

/// Marks a component as one an [`EntityInspector`](
/// crate::elements::EntityInspector) shows. See
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
