//! Walks a component's reflected value into a collapsible hierarchy
//! and renders it.
//!
//! A struct with no [`ReflectInspect`] of its own is not a leaf: its
//! fields become a group, shown under a header that folds them away.
//! Only a registered type stops the walk and becomes an editable row.
//!
//! What a level is made of is structure, and is kept as such: a
//! level's rows are an [`each`] over the entries the walk finds, so a
//! list gaining an item builds one row and leaves the rest alone, and
//! an enum's own fields are a [`keyed`] on its variant, so only they
//! are built again when it switches. Values ride on bound props, so a
//! number scrubbed under a focused input rebuilds nothing.

use std::any::TypeId;

use bevy::asset::UntypedAssetId;
use bevy::ecs::change_detection::Tick;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::reflect::{PartialReflect, ReflectRef, TypeRegistry};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps as _, button, column, frame, label, row, tint,
};
use bevy_fynix::{
    AnyView, Bevy, ScopedExt as _, Signal, View, ViewExt as _, each,
    keyed,
};
use moxie_asset::type_data;

use super::{
    Binding, Field, Owner, ReflectInspect, enums, field_name,
    field_row, variant_picker,
};
use crate::fold::{self, Chevron, Foldable, FoldsOn};
use crate::gaps::{alive, anchored};
use crate::theme::EditorTheme;

/// Which of an inspected entity's sections were folded shut, keyed by
/// component and path. A section absent here is open. Goes when the
/// entity does.
#[derive(Component, Default)]
struct ClosedSections(HashSet<(TypeId, String)>);

/// [`ClosedSections`], for every inspected asset.
#[derive(Resource, Default)]
pub(crate) struct ClosedAssetSections(
    HashSet<(UntypedAssetId, TypeId, String)>,
);

/// Whether the section at `root`/`path` on `owner` is open.
pub(crate) fn section_open(
    world: &World,
    owner: Owner,
    root: TypeId,
    path: &str,
) -> bool {
    let path = path.to_string();
    let closed = match owner {
        Owner::Entity(entity) => {
            world.get::<ClosedSections>(entity).is_some_and(
                |sections| sections.0.contains(&(root, path)),
            )
        }
        Owner::Asset(id) => {
            world.get_resource::<ClosedAssetSections>().is_some_and(
                |sections| sections.0.contains(&(id, root, path)),
            )
        }
    };
    !closed
}

/// Flips the section at `root`/`path` on `owner` open or shut.
pub(crate) fn toggle_section(
    world: &mut World,
    owner: Owner,
    root: TypeId,
    path: String,
    open: bool,
) {
    let entity = match owner {
        Owner::Entity(entity) => entity,
        Owner::Asset(id) => {
            let mut sections = world.get_resource_or_insert_with(
                ClosedAssetSections::default,
            );
            if open {
                sections.0.remove(&(id, root, path));
            } else {
                sections.0.insert((id, root, path));
            }
            return;
        }
    };
    let Ok(mut entity) = world.get_entity_mut(entity) else {
        return;
    };
    match entity.get_mut::<ClosedSections>() {
        Some(mut sections) if !open => {
            sections.0.insert((root, path));
        }
        Some(mut sections) => {
            sections.0.remove(&(root, path));
        }
        None if !open => {
            let mut sections = HashSet::default();
            sections.insert((root, path));
            entity.insert(ClosedSections(sections));
        }
        None => {}
    }
}

/// One row the walk found, as far as what makes it a different row.
/// Paths are relative to the field the walk started at.
#[derive(Clone, PartialEq)]
enum Entry {
    /// A registered editor draws it.
    Leaf {
        path: String,
        name: String,
        type_id: TypeId,
    },
    /// A struct, tuple or list, its fields under a folding header.
    Group { path: String, name: String },
    /// An enum: a variant picker, and the active variant's fields
    /// when it has any.
    Variant {
        path: String,
        name: String,
        /// The active variant is a one-field tuple variant.
        single_tuple_field: bool,
        /// The active variant has fields to show.
        has_children: bool,
    },
}

/// The leaf, enum, and single-field-tuple-struct handling shared by
/// [`push_entry`] and [`push_unnamed`]. `false` leaves a struct, a
/// multi-field tuple struct or a list for the caller.
fn push_common(
    registry: &TypeRegistry,
    value: &dyn PartialReflect,
    path: &str,
    name: &str,
    out: &mut Vec<Entry>,
) -> bool {
    if let Some(type_id) =
        value.get_represented_type_info().map(|i| i.type_id())
        && registry.get_type_data::<ReflectInspect>(type_id).is_some()
    {
        out.push(Entry::Leaf {
            path: path.to_string(),
            name: name.to_string(),
            type_id,
        });
        return true;
    }

    if enums::variants(value).is_some() {
        out.push(Entry::Variant {
            path: path.to_string(),
            name: name.to_string(),
            single_tuple_field: enums::is_single_tuple_variant(value),
            has_children: !variant_children(registry, value, path)
                .is_empty(),
        });
        return true;
    }

    if let ReflectRef::TupleStruct(tuple) = value.reflect_ref()
        && tuple.field_len() == 1
    {
        if let Some(inner) = tuple.field(0) {
            push_unnamed(
                registry,
                inner,
                &join(path, "0"),
                name,
                out,
            );
        }
        return true;
    }

    false
}

/// Whether `value` holds fields of its own to walk.
fn has_fields(value: &dyn PartialReflect) -> bool {
    matches!(
        value.reflect_ref(),
        ReflectRef::Struct(_)
            | ReflectRef::TupleStruct(_)
            | ReflectRef::List(_)
            | ReflectRef::Array(_)
    )
}

/// One field: a leaf if an editor is registered for its type, a
/// collapsible group if it is a struct or list with none of its own,
/// or dropped if it's neither. A single-field tuple struct has no
/// field name to head a group with, so it recurses into that field
/// instead; see [`push_unnamed`].
fn push_entry(
    registry: &TypeRegistry,
    value: &dyn PartialReflect,
    path: &str,
    name: &str,
    out: &mut Vec<Entry>,
) {
    if push_common(registry, value, path, name, out) {
        return;
    }

    if has_fields(value) {
        // An empty struct has nothing to fold away, so it isn't
        // worth a header of its own. An empty list is how a list
        // that is about to gain an item looks, so it keeps one.
        let is_list = matches!(
            value.reflect_ref(),
            ReflectRef::List(_) | ReflectRef::Array(_)
        );
        if is_list
            || !collect_entries(registry, value, path).is_empty()
        {
            out.push(Entry::Group {
                path: path.to_string(),
                name: name.to_string(),
            });
        }
    }
}

/// As [`push_entry`], but for a value with no field name of its own -
/// the sole field of a tuple struct or a one-field tuple enum variant.
/// A struct here is spliced in directly instead of wrapped, the same
/// as the walk's own root.
fn push_unnamed(
    registry: &TypeRegistry,
    value: &dyn PartialReflect,
    path: &str,
    name: &str,
    out: &mut Vec<Entry>,
) {
    if push_common(registry, value, path, name, out) {
        return;
    }

    if has_fields(value) {
        out.extend(collect_entries(registry, value, path));
    }
}

/// A variant's own fields. A one-field tuple variant's sole field is
/// spliced in directly rather than nested under an index; see
/// [`push_unnamed`].
fn variant_children(
    registry: &TypeRegistry,
    value: &dyn PartialReflect,
    path: &str,
) -> Vec<Entry> {
    if enums::is_single_tuple_variant(value)
        && let ReflectRef::Enum(value) = value.reflect_ref()
        && let Some(inner) = value.field_at(0)
    {
        let mut out = Vec::new();
        push_unnamed(registry, inner, &join(path, "0"), "", &mut out);
        return out;
    }
    collect_entries(registry, value, path)
}

/// The entries for `value`'s own fields, one level down from `prefix`.
fn collect_entries(
    registry: &TypeRegistry,
    value: &dyn PartialReflect,
    prefix: &str,
) -> Vec<Entry> {
    let mut out = Vec::new();
    match value.reflect_ref() {
        ReflectRef::Struct(value) => {
            for i in 0..value.field_len() {
                let (Some(name), Some(field)) =
                    (value.name_at(i), value.field_at(i))
                else {
                    continue;
                };
                push_entry(
                    registry,
                    field,
                    &join(prefix, name),
                    name,
                    &mut out,
                );
            }
        }
        ReflectRef::TupleStruct(value) => {
            for i in 0..value.field_len() {
                let Some(field) = value.field(i) else {
                    continue;
                };
                let index = i.to_string();
                push_entry(
                    registry,
                    field,
                    &join(prefix, &index),
                    &index,
                    &mut out,
                );
            }
        }
        // Whatever the active variant carries, named the way a path
        // reaches it - by field for a struct variant, by index for a
        // tuple one.
        ReflectRef::Enum(value) => {
            for i in 0..value.field_len() {
                let Some(field) = value.field_at(i) else {
                    continue;
                };
                let name = value
                    .name_at(i)
                    .map(str::to_string)
                    .unwrap_or_else(|| i.to_string());
                push_entry(
                    registry,
                    field,
                    &join(prefix, &name),
                    &name,
                    &mut out,
                );
            }
        }
        ReflectRef::List(value) => {
            for (i, item) in value.iter().enumerate() {
                push_item(registry, item, prefix, i, &mut out);
            }
        }
        ReflectRef::Array(value) => {
            for (i, item) in value.iter().enumerate() {
                push_item(registry, item, prefix, i, &mut out);
            }
        }
        _ => {}
    }
    out
}

/// The item at `index` of a list or array at `prefix`, named by its
/// index.
fn push_item(
    registry: &TypeRegistry,
    item: &dyn PartialReflect,
    prefix: &str,
    index: usize,
    out: &mut Vec<Entry>,
) {
    push_entry(
        registry,
        item,
        &format!("{prefix}[{index}]"),
        &format!("[{index}]"),
        out,
    );
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// The last path segment, what a row labels itself with when nothing
/// named it. The group above it already said the rest.
fn leaf_name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// `field`'s own editable value, when the whole thing reflects a
/// single, nameless leaf - `Name`, say - rather than a set of fields.
/// Its card's title stands in for that missing name, so it needs the
/// same drag source a genuine field's [`field_name`] label carries.
pub(crate) fn root_leaf(
    world: &World,
    field: &Field,
) -> Option<Field> {
    match entries(world, field).as_slice() {
        [Entry::Leaf { path, name, .. }] if name.is_empty() => {
            Some(field.child(path))
        }
        _ => None,
    }
}

/// The entries at `field`, in walk order.
///
/// `field` itself is never wrapped in a group: a leaf type is the one
/// row shown, and a struct's fields are listed directly, with no
/// header for a group that was never named.
fn entries(world: &World, field: &Field) -> Vec<Entry> {
    let mut out = Vec::new();
    field.read_at(world, |value| {
        // Taken inside the read, where `Field` has already released
        // its own guard.
        let registry = world.resource::<AppTypeRegistry>().read();

        if !push_common(&registry, value, "", "", &mut out) {
            out = collect_entries(&registry, value, "");
        }
    });
    out
}

/// The entries for the fields of the active variant of the enum at
/// `field`.
fn variant_entries(world: &World, field: &Field) -> Vec<Entry> {
    let mut out = Vec::new();
    field.read_at(world, |value| {
        let registry = world.resource::<AppTypeRegistry>().read();
        out = variant_children(&registry, value, "");
    });
    out
}

/// A signal of what `read` makes of `field`, which fires when that
/// differs from the last time.
///
/// The component's tick is checked first, so the read only runs when
/// something touched the component. It goes quiet once `anchor` is
/// despawned; see [`changing_under`](crate::gaps::changing_under).
fn watch<T>(
    anchor: Option<Entity>,
    field: Field,
    read: impl Fn(&World, &Field) -> T + Clone + Send + Sync + 'static,
) -> Signal<T>
where
    T: PartialEq + Send + Sync + 'static,
{
    let reader = field.clone();
    let peek = read.clone();
    let mut seen: Option<T> = None;
    let mut seen_tick: Option<Tick> = None;
    // Whether a write could still land on the tick seen.
    let mut open = false;
    Signal::new(
        move |world: &World| read(world, &reader),
        move |world: &World| {
            if !alive(world, anchor) {
                return false;
            }
            let tick = field.changed_tick(world);
            if seen.is_some() && !open && tick == seen_tick {
                return false;
            }
            seen_tick = tick;
            open = tick == Some(world.read_change_tick());

            let current = peek(world, &field);
            let fired = seen.as_ref() != Some(&current);
            seen = Some(current);
            fired
        },
    )
}

/// The place a section of `field` keeps its fold state.
fn section_key(field: &Field) -> (Owner, TypeId, String) {
    (field.owner(), field.root_type(), field.path().to_string())
}

/// Editable rows for everything reflectable under `root`, which is a
/// whole component at the empty path.
///
/// `depth` is how many [`Foldable`] bodies this sits under, for the
/// rows to keep their columns aligned. `0` for a call site with none
/// of its own.
pub fn inspector_fields(
    root: Field,
    depth: u32,
) -> impl View<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        let walked = root.clone();
        each::<EditorTheme, Entry, Entry>(
            watch(anchor, root, entries),
            |entry| entry.clone(),
            move |entry| entry_view(&walked, entry, depth),
        )
        .within(
            frame()
                .direction(FlexDirection::Column)
                .width(percent(100.0))
                .gap(4.0),
        )
    })
}

/// The fields of the active variant of the enum at `field`, built
/// again when it switches to another.
///
/// An enum's own picker is [`variant_picker`]; this is what goes
/// under it.
pub fn variant_fields(
    field: Field,
    depth: u32,
) -> impl View<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        let walked = field.clone();
        keyed::<EditorTheme, Option<String>>(
            watch(anchor, field, |world, field| {
                field.read_at(world, enums::active_in).flatten()
            }),
            move |_| {
                let field = walked.clone();
                AnyView::<Bevy, EditorTheme>::new(move |cx| {
                    let rows = variant_entries(cx.world, &field)
                        .iter()
                        .map(|entry| entry_view(&field, entry, depth))
                        .collect::<Vec<_>>();
                    cx.build(column(rows).gap(4.0))
                })
            },
        )
        .within(
            frame()
                .direction(FlexDirection::Column)
                .width(percent(100.0))
                .gap(0.0),
        )
    })
}

/// The view of one entry found under `root`.
fn entry_view(
    root: &Field,
    entry: &Entry,
    depth: u32,
) -> AnyView<Bevy, EditorTheme> {
    match entry {
        Entry::Leaf {
            path,
            name,
            type_id,
        } => {
            let field = root.child(path);
            field_row(
                name_label(&field, name),
                leaf_editor(field, *type_id),
                depth,
            )
            .boxed()
        }
        Entry::Group { path, name } => {
            let field = root.child(path);
            section(name.clone(), section_key(&field), move || {
                inspector_fields(field.clone(), depth + 1).boxed()
            })
        }
        Entry::Variant {
            path,
            name,
            single_tuple_field,
            has_children,
        } => variant_entry(
            root,
            path,
            name,
            *single_tuple_field,
            *has_children,
            depth,
        ),
    }
}

/// A row's label, dimmer than the value it labels: the field name is
/// a caption, not the content. `None` for a field with no name.
fn name_label(
    field: &Field,
    name: &str,
) -> Option<AnyView<Bevy, EditorTheme>> {
    (!name.is_empty()).then(|| {
        field_name(Some(field.clone()), name)
            .tone(Tone::Dim)
            .boxed()
    })
}

/// The editor registered for `type_id`, bound to `field`.
fn leaf_editor(
    field: Field,
    type_id: TypeId,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let view =
            match type_data::<ReflectInspect>(cx.world, type_id) {
                Some(editor) => editor.build(Binding::from(field)),
                None => frame().boxed(),
            };
        cx.build(view)
    })
}

/// A variant picker, and the active variant's own fields folded
/// underneath it, if the enum carries data.
fn variant_entry(
    root: &Field,
    path: &str,
    name: &str,
    single_tuple_field: bool,
    has_children: bool,
    depth: u32,
) -> AnyView<Bevy, EditorTheme> {
    let field = root.child(path);
    let picker = variant_picker(Binding::from(field.clone()));

    if !has_children {
        let text = if name.is_empty() {
            leaf_name(path)
        } else {
            name
        };
        return field_row(name_label(&field, text), picker, depth)
            .boxed();
    }

    // Nothing named this - the walk's own root, or a single-field
    // tuple struct spliced into it; see `entries` and `push_common`.
    if name.is_empty() {
        return column((picker, variant_fields(field, depth)))
            .gap(4.0)
            .boxed();
    }

    // A one-field tuple variant needs no header either, just the
    // fold's usual indent under the picker row.
    if single_tuple_field {
        let head = field_row(name_label(&field, name), picker, depth);
        return column((
            head,
            fold::indent(variant_fields(field, depth + 1)),
        ))
        .gap(4.0)
        .boxed();
    }

    let key = section_key(&field);
    let binding = Binding::from(field.clone());
    section(name.to_string(), key, move || {
        column((
            variant_picker(binding.clone()),
            variant_fields(field.clone(), depth + 1),
        ))
        .gap(4.0)
        .boxed()
    })
}

/// A collapsible section: a header that folds it, and a body indented
/// under a guide rail. Whether it is open is kept under `key` (owner,
/// root type, path), so it survives the section being built again.
pub fn section(
    name: String,
    key: (Owner, TypeId, String),
    body: impl Fn() -> AnyView<Bevy, EditorTheme> + Send + Sync + 'static,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let (owner, root, path) = key;
        let open = section_open(cx.world, owner, root, &path);
        let gap = cx.theme().space.sm;
        let header = move |chevron: Chevron| {
            button(
                row((
                    chevron.icon(),
                    label(name).bold(true).wrap(false),
                ))
                .align(AlignItems::Center)
                .gap(gap),
            )
            .width(percent(100.0))
            .justify(JustifyContent::FlexStart)
            .padding(UiRect::axes(px(4.0), px(3.0)))
            .radius(4.0)
            .rules(tint)
        };
        cx.build(
            Foldable::new(header, body)
                .folds_on(FoldsOn::Header)
                .open(open)
                .on_toggle(move |world, open| {
                    toggle_section(
                        world,
                        owner,
                        root,
                        path.clone(),
                        open,
                    );
                }),
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy::ecs::hierarchy::Children;
    use bevy::text::EditableText;
    use bevy::ui_widgets::Activate;

    use super::*;
    use crate::tests::{self, Kind, Probe};

    fn probe_fields(app: &mut App, probe: Entity) -> Entity {
        tests::show(
            app,
            inspector_fields(Field::of::<Probe>(probe), 0),
        )
    }

    fn probe_mut(app: &mut App, probe: Entity) -> Mut<'_, Probe> {
        app.world_mut().get_mut::<Probe>(probe).unwrap()
    }

    fn rows(app: &App, node: Entity) -> Vec<Entity> {
        app.world()
            .get::<Children>(node)
            .map(|kids| kids.iter().collect())
            .unwrap_or_default()
    }

    fn labels(app: &App, node: Entity) -> Vec<String> {
        tests::all::<Text>(app, node)
            .into_iter()
            .map(|node| {
                app.world().get::<Text>(node).unwrap().0.clone()
            })
            .collect()
    }

    #[test]
    fn a_struct_expands_into_one_row_per_field() {
        let (mut app, probe) = tests::probe_app();
        let root = probe_fields(&mut app, probe);

        // One per field of `Probe`, the two groups and the enum
        // among them.
        assert_eq!(rows(&app, root).len(), 9);
        let names = labels(&app, root);
        for name in [
            "on", "level", "name", "time", "offset", "size", "inner",
            "kind", "items", "a", "b",
        ] {
            assert!(
                names.contains(&name.to_string()),
                "{name} in {names:?}"
            );
        }
        // The nested struct's fields are rows of their own.
        assert_eq!(
            tests::all::<EditableText>(&app, root).len(),
            // level, name, time, offset x3, size x2, inner a and b
            10
        );
    }

    #[test]
    fn scrubbing_a_number_rebuilds_nothing() {
        let (mut app, probe) = tests::probe_app();
        let root = probe_fields(&mut app, probe);
        let before = tests::below(&app, root);

        let level = tests::field_root(&app, root);
        tests::drag(&mut app, level, 10.0);
        tests::drag(&mut app, level, 20.0);

        assert_eq!(probe_mut(&mut app, probe).level, 0.2);
        assert_eq!(tests::below(&app, root), before);
    }

    #[test]
    fn an_enum_switches_its_fields_when_the_variant_changes() {
        let (mut app, probe) = tests::probe_app();
        let root = probe_fields(&mut app, probe);
        let top = rows(&app, root);
        let fields = |app: &App| {
            let names = labels(app, root);
            ["radius", "width", "height"]
                .into_iter()
                .filter(|name| names.contains(&name.to_string()))
                .collect::<Vec<_>>()
        };
        assert!(fields(&app).is_empty(), "a unit variant has none");

        probe_mut(&mut app, probe).kind =
            Kind::Circle { radius: 2.0 };
        app.update();
        assert_eq!(fields(&app), ["radius"]);
        let after_circle = rows(&app, root);
        assert_eq!(
            top.iter()
                .zip(&after_circle)
                .filter(|(before, after)| before != after)
                .count(),
            1,
            "only the enum's own row is built again"
        );

        probe_mut(&mut app, probe).kind = Kind::Rect {
            width: 1.0,
            height: 2.0,
        };
        app.update();
        assert_eq!(fields(&app), ["width", "height"]);
        assert_eq!(
            rows(&app, root),
            after_circle,
            "between two variants with fields, its row stays"
        );

        probe_mut(&mut app, probe).kind = Kind::Dot;
        app.update();
        assert!(fields(&app).is_empty());
    }

    #[test]
    fn a_list_grows_by_one_row_without_building_the_others() {
        let (mut app, probe) = tests::probe_app();
        probe_mut(&mut app, probe).items = vec![1.0, 2.0];
        let root = probe_fields(&mut app, probe);
        let inputs = |app: &App| {
            tests::all::<EditableText>(app, root)
                .into_iter()
                .map(|node| {
                    app.world()
                        .get::<EditableText>(node)
                        .unwrap()
                        .value()
                        .to_string()
                })
                .collect::<Vec<_>>()
        };
        let before = tests::below(&app, root);
        assert_eq!(inputs(&app).len(), 10 + 2);
        assert!(labels(&app, root).contains(&"[1]".to_string()));

        probe_mut(&mut app, probe).items.push(3.0);
        app.update();

        assert_eq!(inputs(&app).len(), 10 + 3);
        assert!(labels(&app, root).contains(&"[2]".to_string()));
        let after = tests::below(&app, root);
        assert!(
            before.iter().all(|node| after.contains(node)),
            "every node there before is still there"
        );
    }

    #[test]
    fn a_section_remembers_being_shut() {
        let (mut app, probe) = tests::probe_app();
        let root = probe_fields(&mut app, probe);
        let header =
            tests::all::<bevy::ui_widgets::Button>(&app, root)[0];

        app.world_mut().trigger(Activate { entity: header });
        app.update();

        assert!(!section_open(
            app.world(),
            Owner::Entity(probe),
            TypeId::of::<Probe>(),
            "inner"
        ));
        assert!(section_open(
            app.world(),
            Owner::Entity(probe),
            TypeId::of::<Probe>(),
            "kind"
        ));
    }

    #[test]
    fn a_nameless_leaf_is_a_root_leaf() {
        let mut app = tests::app();
        let named = app.world_mut().spawn(Name::new("cube")).id();
        let probe = app.world_mut().spawn(Probe::default()).id();

        assert_eq!(
            root_leaf(app.world(), &Field::of::<Name>(named)),
            Some(Field::of::<Name>(named))
        );
        assert_eq!(
            root_leaf(app.world(), &Field::of::<Probe>(probe)),
            None
        );
    }
}
