//! Picking an enum's variant.
//!
//! Not a registered [`Inspect`](super::Inspect) widget: which variants
//! a type has is something reflection already knows, so this is
//! dispatched on the shape of the value rather than on its type. That
//! is also what lets it serve enums from crates the inspector cannot
//! name.
//!
//! Switching into a variant that carries data means inventing that
//! data: a unit variant needs none, and a variant with fields is
//! constructible when every one of its field types has a registered
//! [`ReflectDefault`]. An enum with any variant that isn't - some
//! field type nothing derives `Default` for - is shown read-only, at
//! whichever variant it is already on, with that variant's fields
//! walked underneath like a struct's.

use bevy::prelude::*;
use bevy::reflect::enums::{
    DynamicEnum, DynamicVariant, VariantInfo, VariantType,
};
use bevy::reflect::std_traits::ReflectDefault;
use bevy::reflect::structs::DynamicStruct;
use bevy::reflect::tuple::DynamicTuple;
use bevy::reflect::{
    PartialReflect, ReflectRef, TypeInfo, TypeRegistry,
};
use bevy_fynix::tag::TagExt as _;

use fynix::composer::Composer;
use fynix::prelude::*;

use super::{Source, when_changed};
use crate::elements::{
    Dropdown, DropdownCursor, DropdownList, DropdownMenu, Frame,
    Icon, Label, LabelCursor, menu_item,
};
use crate::icons;
use crate::reactive::{BevyUi, FynixHost};

/// Every variant of `value`'s type, if it is an enum at all.
///
/// Read off the type, so the choices do not change with whichever
/// variant happens to be active.
pub(super) fn variants(
    value: &dyn PartialReflect,
) -> Option<Vec<String>> {
    if !matches!(value.reflect_ref(), ReflectRef::Enum(_)) {
        return None;
    }

    let TypeInfo::Enum(info) = value.get_represented_type_info()?
    else {
        return None;
    };
    Some(info.variant_names().iter().map(|n| n.to_string()).collect())
}

/// Whether `value`'s active variant is a one-field tuple variant, the
/// enum counterpart of a single-field tuple struct.
pub(super) fn is_single_tuple_variant(
    value: &dyn PartialReflect,
) -> bool {
    let ReflectRef::Enum(value) = value.reflect_ref() else {
        return false;
    };
    value.variant_type() == VariantType::Tuple
        && value.field_len() == 1
}

/// Whether every variant of `value`'s type can be switched into.
pub(super) fn constructible(
    value: &dyn PartialReflect,
    registry: &TypeRegistry,
) -> bool {
    let Some(TypeInfo::Enum(info)) =
        value.get_represented_type_info()
    else {
        return false;
    };

    info.iter().all(|variant| defaultable(variant, registry))
}

/// A unit variant needs nothing; a variant with fields needs every
/// field's type to carry a [`ReflectDefault`].
fn defaultable(
    variant: &VariantInfo,
    registry: &TypeRegistry,
) -> bool {
    match variant {
        VariantInfo::Unit(_) => true,
        VariantInfo::Struct(info) => info
            .iter()
            .all(|field| has_default(field.type_id(), registry)),
        VariantInfo::Tuple(info) => info
            .iter()
            .all(|field| has_default(field.type_id(), registry)),
    }
}

fn has_default(
    type_id: core::any::TypeId,
    registry: &TypeRegistry,
) -> bool {
    registry.get_type_data::<ReflectDefault>(type_id).is_some()
}

/// `name` as a [`DynamicVariant`] of `value`'s type, its fields (if
/// any) filled from their own [`ReflectDefault`]. `None` if the
/// variant isn't constructible - a caller only reaches this from a
/// picker [`constructible`] already gated, so that should not happen.
fn constructed(
    value: &dyn PartialReflect,
    registry: &TypeRegistry,
    name: &str,
) -> Option<DynamicVariant> {
    let TypeInfo::Enum(info) = value.get_represented_type_info()?
    else {
        return None;
    };

    Some(match info.variant(name)? {
        VariantInfo::Unit(_) => DynamicVariant::Unit,
        VariantInfo::Struct(info) => {
            let mut fields = DynamicStruct::default();
            for field in info.iter() {
                let default = registry
                    .get_type_data::<ReflectDefault>(field.type_id())?
                    .default();
                fields.insert_boxed(
                    field.name(),
                    default.into_partial_reflect(),
                );
            }
            DynamicVariant::Struct(fields)
        }
        VariantInfo::Tuple(info) => {
            let mut fields = DynamicTuple::default();
            for field in info.iter() {
                let default = registry
                    .get_type_data::<ReflectDefault>(field.type_id())?
                    .default();
                fields.insert_boxed(default.into_partial_reflect());
            }
            DynamicVariant::Tuple(fields)
        }
    })
}

/// Which variant `source` is on.
fn active(source: &dyn Source, world: &World) -> Option<String> {
    let value = source.get(world)?;
    let ReflectRef::Enum(value) = value.reflect_ref() else {
        return None;
    };
    Some(value.variant_name().to_string())
}

/// The variant, as a dropdown over the rest.
///
/// `pick` is false when some variant of the type isn't
/// [`constructible`], which then only names where it stands. The two
/// look nothing alike, so they share a [`Frame`] and this comes back
/// the same either way.
pub(super) struct VariantPicker<'a> {
    pub source: &'a dyn Source,
    pub variants: Vec<String>,
    pub pick: bool,
}

impl Composer<FynixHost> for VariantPicker<'_> {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let Self {
            source,
            variants,
            pick,
        } = self;

        let current = active(source, ui.world)
            .unwrap_or_else(|| "-".to_string());
        let source = source.boxed();
        // Sized to the longest variant, so picking another does not
        // resize the row.
        let width = Dropdown::width_for(&variants, 12.0);

        ui.elem(elem!(Frame, align = AlignItems::Center))
            .with(move |ui| {
                if !pick {
                    name(ui, &*source, current);
                    return;
                }

                ui.elem(elem!(DropdownMenu)).with(move |ui| {
                    control(ui, &*source, current, width);
                    list(ui, &*source, variants, width);
                });
            })
            .handle()
    }
}

/// Just the active variant, for an enum that cannot be moved.
fn name(ui: &mut BevyUi, source: &dyn Source, current: String) {
    let shown = source.boxed();
    let text = ui.theme.color.text;

    ui.elem(elem!(Label, text = current, wrap = false, color = text))
        .bind(
            |label| label.text(),
            when_changed(source),
            move |WorldNodeRef { world, .. }| {
                active(&*shown, world).unwrap_or_default()
            },
        );
}

/// The shut control, showing whichever variant is active.
fn control(
    ui: &mut BevyUi,
    source: &dyn Source,
    current: String,
    width: Val,
) {
    let shown = source.boxed();
    let text = ui.theme.color.text;
    let text_dim = ui.theme.color.text_dim;

    ui.elem(elem!(
        Dropdown,
        min_width = width,
        max_width = width,
        label =
            elem!(Label, text = current, wrap = false, color = text),
        chevron = elem!(
            Icon,
            image = icons::CHEVRON,
            color = text_dim,
            size = px(9),
            rotation = 180.0f32
        )
    ))
    .pointer_tags()
    .bind(
        |dropdown| dropdown.label().text(),
        when_changed(source),
        move |WorldNodeRef { world, .. }| {
            active(&*shown, world).unwrap_or_default()
        },
    );
}

/// One row per variant. The list closes itself once one is picked.
fn list(
    ui: &mut BevyUi,
    source: &dyn Source,
    variants: Vec<String>,
    width: Val,
) {
    let source = source.boxed();

    ui.elem(elem!(DropdownList, width = width)).with(move |ui| {
        for variant in variants {
            option(ui, &*source, variant);
        }
    });
}

/// A boxed [`Source`] cloned through [`Source::boxed`] rather than
/// derived - a trait object isn't `Clone` on its own - so `option`'s
/// row can hand [`menu_item`] a closure it's free to run more than
/// once.
struct ClonableSource(Box<dyn Source>);

impl Clone for ClonableSource {
    fn clone(&self) -> Self {
        Self(self.0.boxed())
    }
}

impl ClonableSource {
    // Methods of its own: a closure only using the field captures
    // just that field - `Box<dyn Source>` on its own, which isn't
    // `Clone`.
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>> {
        self.0.get(world)
    }

    fn set(&self, world: &mut World, value: &dyn PartialReflect) {
        self.0.set(world, value);
    }
}

fn option(ui: &mut BevyUi, source: &dyn Source, variant: String) {
    let source = ClonableSource(source.boxed());

    menu_item(ui, None, variant.clone(), move |world| {
        let Some(value) = source.get(world) else {
            return;
        };
        let dynamic = {
            let registry = world.resource::<AppTypeRegistry>().read();
            constructed(&*value, &registry, &variant)
        };
        if let Some(dynamic) = dynamic {
            source.set(
                world,
                &DynamicEnum::new(variant.clone(), dynamic),
            );
        }
    });
}
