//! Picking an enum's variant.
//!
//! Dispatched on the shape of the value, so it serves enums from
//! crates the inspector cannot name.
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
use bevy_fynix::views::{FrameProps as _, dropdown, frame, label};
use bevy_fynix::{AnyView, Bevy, Cx, ViewExt as _};

use super::Binding;
use crate::icons;
use crate::theme::EditorTheme;

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

/// The name of the variant `binding` is on.
pub(super) fn active(
    binding: &Binding,
    world: &World,
) -> Option<String> {
    active_in(&*binding.get(world)?)
}

/// The name of the variant `value` is on, if it is an enum.
pub(super) fn active_in(
    value: &dyn PartialReflect,
) -> Option<String> {
    let ReflectRef::Enum(value) = value.reflect_ref() else {
        return None;
    };
    Some(value.variant_name().to_string())
}

/// Switches `binding` to `variant`, with whatever data it carries
/// made up from defaults.
fn switch(world: &mut World, binding: &Binding, variant: &str) {
    let Some(value) = binding.get(world) else {
        return;
    };
    let dynamic = {
        let registry = world.resource::<AppTypeRegistry>().read();
        constructed(&*value, &registry, variant)
    };
    if let Some(dynamic) = dynamic {
        binding.set(world, &DynamicEnum::new(variant, dynamic));
    }
}

/// The width of a dropdown over `variants`, fitting the longest one,
/// its padding and the chevron, so picking another keeps the row's
/// size.
fn width_for(variants: &[String], theme: &EditorTheme) -> f32 {
    const GLYPH: f32 = 0.6;
    const CHEVRON: f32 = 16.0;
    let longest = variants
        .iter()
        .map(|variant| variant.chars().count())
        .max()
        .unwrap_or(0);
    longest as f32 * theme.text.body * GLYPH
        + theme.space.md * 2.0
        + CHEVRON
}

/// The variant `binding` is on, as a dropdown over the rest.
///
/// When some variant of the type can't be switched into it only
/// names where it stands. Anything that is not an enum is an empty
/// node. Which of the two is settled when this is built.
pub fn variant_picker(
    binding: Binding,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let value = binding.get(cx.world);
        let options = value.as_deref().and_then(|value| {
            let variants = variants(value)?;
            let registry =
                cx.world.resource::<AppTypeRegistry>().read();
            Some((variants, constructible(value, &registry)))
        });
        let view = match options {
            Some((variants, true)) => {
                pick(binding, variants, cx.theme())
            }
            Some((_, false)) => {
                label(binding.derive(|binding, world| {
                    active(binding, world).unwrap_or_default()
                }))
                .wrap(false)
                .boxed()
            }
            None => frame().boxed(),
        };
        cx.build(view)
    })
}

/// A dropdown over `variants`, showing the active one.
fn pick(
    binding: Binding,
    variants: Vec<String>,
    theme: &EditorTheme,
) -> AnyView<Bevy, EditorTheme> {
    let width = px(width_for(&variants, theme));
    let selected = {
        let variants = variants.clone();
        binding.derive(move |binding, world| {
            active(binding, world)
                .and_then(|name| {
                    variants
                        .iter()
                        .position(|variant| *variant == name)
                })
                .unwrap_or(0)
        })
    };
    let names = variants.clone();
    AnyView::new(move |cx: &mut Cx<'_, Bevy, EditorTheme>| {
        let chevron =
            cx.world.resource::<AssetServer>().load(icons::CHEVRON);
        cx.build(
            dropdown(
                variants,
                selected,
                chevron,
                move |world, at| {
                    if let Some(name) = names.get(at) {
                        switch(world, &binding, name);
                    }
                },
            )
            .min_width(width)
            .max_width(width),
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy::ui_widgets::Activate;

    use super::*;
    use crate::inspector::Field;
    use crate::tests::{self, Kind, Probe};

    fn kind_picker(app: &mut App, probe: Entity) -> Entity {
        let binding =
            Binding::from(Field::of::<Probe>(probe).child("kind"));
        tests::show(app, variant_picker(binding))
    }

    fn kind(app: &App, probe: Entity) -> Kind {
        app.world().get::<Probe>(probe).unwrap().kind.clone()
    }

    /// The label in the shut control, which comes first.
    fn shown(app: &App, root: Entity) -> String {
        let first = tests::all::<Text>(app, root)[0];
        app.world().get::<Text>(first).unwrap().0.clone()
    }

    #[test]
    fn it_lists_every_variant_and_shows_the_active_one() {
        let (mut app, probe) = tests::probe_app();
        let root = kind_picker(&mut app, probe);
        assert_eq!(shown(&app, root), "Dot");

        tests::open_menus(&mut app, root);
        let texts = tests::all::<Text>(&app, root)
            .into_iter()
            .map(|node| {
                app.world().get::<Text>(node).unwrap().0.clone()
            })
            .filter(|text| text != "v")
            .collect::<Vec<_>>();
        assert_eq!(texts, ["Dot", "Dot", "Circle", "Rect"]);
    }

    #[test]
    fn it_follows_the_world_and_picking_writes_it() {
        let (mut app, probe) = tests::probe_app();
        let root = kind_picker(&mut app, probe);

        app.world_mut().get_mut::<Probe>(probe).unwrap().kind =
            Kind::Circle { radius: 2.0 };
        app.update();
        assert_eq!(shown(&app, root), "Circle");

        tests::open_menus(&mut app, root);
        let rows =
            tests::all::<bevy::ui_widgets::MenuItem>(&app, root);
        app.world_mut().trigger(Activate { entity: rows[2] });
        app.update();

        assert_eq!(
            kind(&app, probe),
            Kind::Rect {
                width: 0.0,
                height: 0.0
            },
            "its fields made up from defaults"
        );
        assert_eq!(shown(&app, root), "Rect");
    }

    #[test]
    fn a_value_that_is_not_an_enum_is_an_empty_node() {
        let (mut app, probe) = tests::probe_app();
        let binding =
            Binding::from(Field::of::<Probe>(probe).child("level"));
        let root = tests::show(&mut app, variant_picker(binding));
        assert!(tests::below(&app, root).is_empty());
    }
}
