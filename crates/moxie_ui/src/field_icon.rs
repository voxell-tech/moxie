//! Icons bound to a reflected field, for wherever the editor names one.
//!
//! Bind one with the [`FieldIcon`] reflect attribute, or with
//! [`FieldIconAppExt::register_field_icon`] for a type that can't
//! carry it. [`field_icon`] finds the one that applies to a field.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::reflect::{
    GetTypeRegistration, TypeInfo, TypeRegistry, TypeRegistryArc,
};
use bevy_motiongfx::motiongfx::field_path::field::Field;

/// Binds an icon asset path to the field, or the whole type, it is
/// written on:
///
/// ```ignore
/// #[derive(Component, Reflect)]
/// #[reflect(Component, @FieldIcon("icons/editor/translate.png"))]
/// struct Velocity { ... }
/// ```
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldIcon(pub &'static str);

/// Icons registered from outside a type, keyed by field path. Lives
/// on that type's registration.
#[derive(Clone, Default)]
struct FieldIcons(HashMap<String, &'static str>);

/// Registering field icons on the app.
pub trait FieldIconAppExt {
    /// Binds `icon` to `field`: `field!(<Transform>::translation)`.
    /// `field!(<Transform>)` is the type itself.
    ///
    /// Wins over a [`FieldIcon`] attribute on the same field.
    fn register_field_icon<S: Reflect + GetTypeRegistration, T>(
        &mut self,
        field: Field<S, T>,
        icon: &'static str,
    ) -> &mut Self;
}

impl FieldIconAppExt for App {
    fn register_field_icon<S: Reflect + GetTypeRegistration, T>(
        &mut self,
        field: Field<S, T>,
        icon: &'static str,
    ) -> &mut Self {
        self.register_type::<S>();
        let registry: TypeRegistryArc =
            self.world().resource::<AppTypeRegistry>().0.clone();
        let mut registry = registry.write();
        if let Some(registration) =
            registry.get_mut(std::any::TypeId::of::<S>())
        {
            if registration.data::<FieldIcons>().is_none() {
                registration.insert(FieldIcons::default());
            }
            if let Some(icons) = registration.data_mut::<FieldIcons>()
            {
                icons.0.insert(field.field_path().to_owned(), icon);
            }
        }
        self
    }
}

/// The icon for the field at `field_path` of the type at `type_path`:
/// the field's own, else the nearest parent's, up to the type itself.
/// `None` when nothing on the way binds one.
///
/// A [`FieldIcon`] on a field's type applies wherever that type is
/// used as a field, unless the field carries one of its own.
pub fn field_icon(
    registry: &TypeRegistry,
    type_path: &str,
    field_path: &str,
) -> Option<&'static str> {
    let registration = registry.get_with_type_path(type_path)?;
    let registered = registration.data::<FieldIcons>();
    let segments = segments(field_path);

    // The attribute at each depth, the type itself first.
    let mut info = Some(registration.type_info());
    let mut attributed = vec![info.and_then(type_icon)];
    for segment in &segments {
        let step =
            info.and_then(|info| step(registry, info, segment));
        info = step.and_then(|(_, next)| next);
        attributed.push(step.and_then(|(icon, _)| icon));
    }

    (0..=segments.len()).rev().find_map(|depth| {
        registered
            .and_then(|icons| icons.0.get(&join(&segments[..depth])))
            .copied()
            .or(attributed[depth])
    })
}

/// Into `info` by one path `segment`: the icon that field binds
/// (its own, else its type's) and the type it holds.
fn step(
    registry: &TypeRegistry,
    info: &'static TypeInfo,
    segment: &str,
) -> Option<(Option<&'static str>, Option<&'static TypeInfo>)> {
    let (own, type_id, type_info) = match info {
        TypeInfo::Struct(info) => {
            let field = info.field(segment)?;
            (
                field.get_attribute::<FieldIcon>(),
                field.type_id(),
                field.type_info(),
            )
        }
        TypeInfo::TupleStruct(info) => {
            let field =
                info.field_at(segment.parse::<usize>().ok()?)?;
            (
                field.get_attribute::<FieldIcon>(),
                field.type_id(),
                field.type_info(),
            )
        }
        TypeInfo::Tuple(info) => {
            let field =
                info.field_at(segment.parse::<usize>().ok()?)?;
            (
                field.get_attribute::<FieldIcon>(),
                field.type_id(),
                field.type_info(),
            )
        }
        _ => return None,
    };

    let next = type_info
        .or_else(|| registry.get(type_id).map(|r| r.type_info()));
    let icon =
        own.map(|icon| icon.0).or_else(|| next.and_then(type_icon));
    Some((icon, next))
}

/// What a [`FieldIcon`] on the type itself binds.
fn type_icon(info: &'static TypeInfo) -> Option<&'static str> {
    match info {
        TypeInfo::Struct(info) => info.get_attribute::<FieldIcon>(),
        TypeInfo::TupleStruct(info) => {
            info.get_attribute::<FieldIcon>()
        }
        TypeInfo::Enum(info) => info.get_attribute::<FieldIcon>(),
        _ => None,
    }
    .map(|icon| icon.0)
}

fn segments(path: &str) -> Vec<&str> {
    path.split("::")
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn join(segments: &[&str]) -> String {
    if segments.is_empty() {
        String::new()
    } else {
        format!("::{}", segments.join("::"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_motiongfx::motiongfx::field_path::field;

    #[derive(Reflect)]
    struct Inner {
        #[reflect(@FieldIcon("inner.x"))]
        x: f32,
        y: f32,
    }

    #[derive(Reflect)]
    #[reflect(@FieldIcon("marked"))]
    struct Marked {
        value: f32,
    }

    #[derive(Reflect)]
    struct Outer {
        #[reflect(@FieldIcon("outer.inner"))]
        inner: Inner,
        plain: Inner,
        marked: Marked,
        #[reflect(@FieldIcon("field wins"))]
        override_marked: Marked,
        #[reflect(@FieldIcon("tuple"))]
        tuple: (f32, f32),
    }

    #[derive(Reflect)]
    #[reflect(@FieldIcon("root"))]
    struct Rooted {
        a: Inner,
    }

    #[derive(Reflect)]
    struct Bare {
        a: f32,
    }

    fn registry() -> TypeRegistry {
        let mut registry = TypeRegistry::default();
        registry.register::<Outer>();
        registry.register::<Inner>();
        registry.register::<Marked>();
        registry.register::<Rooted>();
        registry.register::<Bare>();
        registry.register::<f32>();
        registry
    }

    fn icon(
        registry: &TypeRegistry,
        type_path: &str,
        path: &str,
    ) -> Option<&'static str> {
        field_icon(registry, type_path, path)
    }

    fn outer() -> &'static str {
        <Outer as bevy::reflect::TypePath>::type_path()
    }

    #[test]
    fn field_attribute_wins_over_parent() {
        let registry = registry();
        assert_eq!(
            icon(&registry, outer(), "::inner::x"),
            Some("inner.x")
        );
    }

    #[test]
    fn unbound_field_falls_back_to_parent() {
        let registry = registry();
        assert_eq!(
            icon(&registry, outer(), "::inner::y"),
            Some("outer.inner")
        );
    }

    #[test]
    fn falls_back_to_the_type_itself() {
        let registry = registry();
        let path = <Rooted as bevy::reflect::TypePath>::type_path();
        assert_eq!(icon(&registry, path, "::a::y"), Some("root"));
        assert_eq!(icon(&registry, path, ""), Some("root"));
    }

    #[test]
    fn nothing_bound_is_none() {
        let registry = registry();
        let path = <Bare as bevy::reflect::TypePath>::type_path();
        assert_eq!(icon(&registry, path, "::a"), None);
        assert_eq!(icon(&registry, outer(), "::plain::y"), None);
    }

    #[test]
    fn unknown_path_or_type_is_none() {
        let registry = registry();
        assert_eq!(icon(&registry, outer(), "::nope"), None);
        assert_eq!(icon(&registry, "not::a::Type", "::a"), None);
    }

    #[test]
    fn unknown_field_still_inherits_from_its_parent() {
        let registry = registry();
        assert_eq!(
            icon(&registry, outer(), "::inner::nope"),
            Some("outer.inner")
        );
    }

    #[test]
    fn a_fields_type_attribute_applies_unless_the_field_has_its_own()
    {
        let registry = registry();
        assert_eq!(
            icon(&registry, outer(), "::marked::value"),
            Some("marked")
        );
        assert_eq!(
            icon(&registry, outer(), "::override_marked::value"),
            Some("field wins")
        );
    }

    #[test]
    fn walks_into_a_tuple() {
        let registry = registry();
        assert_eq!(
            icon(&registry, outer(), "::tuple::1"),
            Some("tuple")
        );
    }

    #[test]
    fn registered_icon_wins_over_attribute() {
        let mut app = App::new();
        app.register_type::<Outer>()
            .register_type::<Inner>()
            .register_type::<f32>()
            .register_field_icon(
                field!(<Outer>::inner::x),
                "registered",
            )
            .register_field_icon(field!(<Outer>::plain), "plain");
        let registry =
            app.world().resource::<AppTypeRegistry>().read();
        assert_eq!(
            icon(&registry, outer(), "::inner::x"),
            Some("registered")
        );
        assert_eq!(
            icon(&registry, outer(), "::plain::y"),
            Some("plain")
        );
    }
}
