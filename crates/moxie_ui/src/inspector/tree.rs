//! Walks a component's reflected value into a collapsible hierarchy
//! and renders it.
//!
//! A struct with no [`ReflectInspect`] of its own is not a leaf: its
//! fields become a group, shown under a header that folds them away.
//! Only a registered type stops the walk and becomes an editable row.

use std::any::TypeId;

use bevy::ecs::change_detection::Tick;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::reflect::{PartialReflect, ReflectRef, TypeRegistry};
use fynix::composer::Composer;
use fynix::prelude::*;
use fynix::records::BuildFn;

use super::field_drag::{FieldStageSource, StagedFieldEdit};
use super::{Field, FieldRow, ReflectInspect, enums};
use crate::elements::{Button, Frame, Icon, Label, TintButton};
use crate::fold::{self, CHEVRON_SHUT, Foldable, FoldsOn};
use crate::icons;
use crate::reactive::{BevyUi, FynixHost};

/// Which of an inspected entity's sections were folded shut, keyed by
/// component and path. A section absent here is open. Goes when the
/// entity does.
#[derive(Component, Default)]
struct ClosedSections(HashSet<(TypeId, String)>);

/// Whether the section at `component`/`path` on `entity` is open.
pub(crate) fn section_open(
    world: &World,
    entity: Entity,
    component: TypeId,
    path: &str,
) -> bool {
    !world.get::<ClosedSections>(entity).is_some_and(|sections| {
        sections.0.contains(&(component, path.to_string()))
    })
}

/// Flips the section at `component`/`path` on `entity` open or shut.
pub(crate) fn toggle_section(
    world: &mut World,
    entity: Entity,
    component: TypeId,
    path: String,
    open: bool,
) {
    let Ok(mut entity) = world.get_entity_mut(entity) else {
        return;
    };
    match entity.get_mut::<ClosedSections>() {
        Some(mut sections) if !open => {
            sections.0.insert((component, path));
        }
        Some(mut sections) => {
            sections.0.remove(&(component, path));
        }
        None if !open => {
            let mut sections = HashSet::default();
            sections.insert((component, path));
            entity.insert(ClosedSections(sections));
        }
        None => {}
    }
}

/// One row the walk found.
#[derive(Clone, PartialEq)]
enum Entry {
    /// A registered widget draws it.
    Leaf {
        path: String,
        name: String,
        type_id: TypeId,
    },
    /// A struct, its fields under a folding header.
    Group {
        path: String,
        name: String,
        children: Vec<Entry>,
    },
    /// An enum: a variant picker, and the active variant's fields
    /// when it has any.
    Variant {
        path: String,
        name: String,
        variants: Vec<String>,
        /// Only unit variants can be picked; see [`enums`].
        pick: bool,
        /// The active variant is a one-field tuple variant.
        single_tuple_field: bool,
        children: Vec<Entry>,
    },
}

/// The leaf, enum, and single-field-tuple-struct handling shared by
/// [`push_entry`] and [`push_unnamed`]. `false` leaves a struct or
/// multi-field tuple struct for the caller.
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

    if let Some(variants) = enums::variants(value) {
        let pick = enums::constructible(value, registry);
        out.push(Entry::Variant {
            path: path.to_string(),
            name: name.to_string(),
            variants,
            pick,
            single_tuple_field: enums::is_single_tuple_variant(value),
            children: variant_children(registry, value, path),
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

/// One field: a leaf if a widget is registered for its type, a
/// collapsible group if it is a struct with none of its own, or
/// dropped if it's neither. A single-field tuple struct has no field
/// name to head a group with, so it recurses into that field instead;
/// see [`push_unnamed`].
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

    if matches!(
        value.reflect_ref(),
        ReflectRef::Struct(_) | ReflectRef::TupleStruct(_)
    ) {
        let children = collect_entries(registry, value, path);
        // An empty struct has nothing to fold away, so it isn't worth
        // a header of its own.
        if !children.is_empty() {
            out.push(Entry::Group {
                path: path.to_string(),
                name: name.to_string(),
                children,
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

    if matches!(
        value.reflect_ref(),
        ReflectRef::Struct(_) | ReflectRef::TupleStruct(_)
    ) {
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
        _ => {}
    }
    out
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// The last path segment, what a row labels itself with. The group
/// above it already said the rest.
fn leaf_name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// `field`'s own editable value, when the whole thing reflects a
/// single, nameless leaf - `Name`, say - rather than a set of fields.
/// Its card's title stands in for that missing name, so it needs the
/// same drag source a genuine field's [`FieldName`](super::FieldName)
/// label carries.
pub(crate) fn root_leaf(
    world: &World,
    field: &Field,
) -> Option<Field> {
    match entries(world, field).as_slice() {
        [Entry::Leaf { path, name, .. }] if name.is_empty() => {
            Some(if path.is_empty() {
                field.clone()
            } else {
                field.child(path)
            })
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

/// Fires when the *shape* under `field` changes: its set of entries -
/// and also when [`StagedFieldEdit`] does, since which leaves toggled
/// changes which `Source` a row's widget is built against, a decision
/// `build_leaf` only makes at build time.
///
/// Values ride on bindings, so a focused number input survives a
/// value change; a rebuild would despawn it mid-edit. The
/// tick is checked first so the walk only runs when something
/// touched the component.
fn shape_changed(
    field: Field,
) -> impl for<'w> FnMut(WorldNodeRef<'w, FynixHost>) -> bool {
    let mut seen_tick: Option<Tick> = None;
    let mut seen_shape: Option<Vec<Entry>> = None;
    let mut seen_staged: Option<Tick> = None;
    move |WorldNodeRef { world, .. }| {
        let staged_tick = world
            .get_resource_change_ticks::<StagedFieldEdit>()
            .map(|ticks| ticks.changed);
        let staged_fired = seen_staged != staged_tick;
        seen_staged = staged_tick;

        let tick = field.changed_tick(world);
        if seen_shape.is_some() && tick == seen_tick {
            return staged_fired;
        }
        seen_tick = tick;

        let current = entries(world, &field);
        let fired = seen_shape.as_ref() != Some(&current);
        seen_shape = Some(current);
        fired || staged_fired
    }
}

/// Editable rows for everything reflectable under `root`, which is a
/// whole component at the empty path.
pub struct InspectorFields {
    pub root: Field,
    /// How many [`Foldable`] bodies this sits under, for `FieldRow`
    /// to keep its columns aligned. `0` for a call site with none of
    /// its own.
    pub depth: u32,
}

impl Composer<FynixHost> for InspectorFields {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let walked = self.root.clone();
        let depth = self.depth;

        ui.elem(elem!(
            Frame,
            width = percent(100),
            direction = FlexDirection::Column,
            row_gap = px(4)
        ))
        .watch(shape_changed(self.root), move |ui| {
            build_entries(
                ui,
                &walked,
                entries(ui.world, &walked),
                depth,
            );
        })
        .handle()
    }
}

fn build_entries(
    ui: &mut BevyUi,
    root: &Field,
    entries: Vec<Entry>,
    depth: u32,
) {
    for entry in entries {
        match entry {
            Entry::Leaf {
                path,
                name,
                type_id,
            } => build_leaf(ui, root, path, name, type_id, depth),
            Entry::Group { path, name, .. } => {
                build_group(ui, root, path, name, depth)
            }
            Entry::Variant {
                path,
                name,
                variants,
                pick,
                single_tuple_field,
                children,
            } => build_variant(
                ui,
                root,
                path,
                name,
                variants,
                pick,
                single_tuple_field,
                children,
                depth,
            ),
        }
    }
}

fn build_leaf(
    ui: &mut BevyUi,
    root: &Field,
    path: String,
    name: String,
    type_id: TypeId,
    depth: u32,
) {
    let drawer = {
        let registry = ui.world.resource::<AppTypeRegistry>().read();
        registry.get_type_data::<ReflectInspect>(type_id).cloned()
    };
    let Some(drawer) = drawer else { return };

    // Dimmer than the value it labels: the field name is a caption,
    // not the content.
    let muted = ui.theme.color.text_dim;
    let label = name;
    let field = root.child(&path);

    let staged =
        ui.world.resource::<StagedFieldEdit>().is_active(&field);
    let stage_source = staged
        .then(|| {
            ui.world
                .resource::<FieldStageSource>()
                .resolve(ui.world, &field)
        })
        .flatten();

    ui.compose(FieldRow {
        label,
        color: muted,
        bold: false,
        depth,
        field: Some(field.clone()),
        value: move |ui: &mut BevyUi| match &stage_source {
            Some(source) => drawer.build(&**source, ui),
            None => drawer.build(&field, ui),
        },
    });
}

/// A variant picker, and the active variant's own fields folded
/// underneath it, if the enum carries data.
fn build_variant(
    ui: &mut BevyUi,
    root: &Field,
    path: String,
    name: String,
    variants: Vec<String>,
    pick: bool,
    single_tuple_field: bool,
    children: Vec<Entry>,
    depth: u32,
) {
    let field = root.child(&path);
    let muted = ui.theme.color.text_dim;

    if children.is_empty() {
        let label = leaf_name(&path).to_string();
        ui.compose(FieldRow {
            label,
            color: muted,
            bold: false,
            depth,
            field: Some(field.clone()),
            value: move |ui: &mut BevyUi| {
                ui.compose(enums::VariantPicker {
                    source: &field,
                    variants,
                    pick,
                });
            },
        });
        return;
    }

    // Nothing named this - the walk's own root, or a single-field
    // tuple struct spliced into it; see `entries` and `push_common`.
    if name.is_empty() {
        ui.compose(enums::VariantPicker {
            source: &field,
            variants,
            pick,
        });
        build_entries(ui, root, children, depth);
        return;
    }

    // A one-field tuple variant needs no header either, just the
    // fold's usual indent under the picker row.
    if single_tuple_field {
        let label = name;
        ui.compose(FieldRow {
            label,
            color: muted,
            bold: false,
            depth,
            field: Some(field.clone()),
            value: move |ui: &mut BevyUi| {
                ui.compose(enums::VariantPicker {
                    source: &field,
                    variants,
                    pick,
                });
            },
        });

        let root = root.clone();
        fold::indent(ui, None, move |ui| {
            build_entries(ui, &root, children.clone(), depth + 1)
        });
        return;
    }

    // Re-walked from `field` on every open rather than carried in the
    // closure, so a section reopened many times never clones stale
    // data forward. `entries` never wraps `field` itself, so its one
    // entry is always this same variant.
    ui.compose(Section::new(
        name,
        (root.entity(), root.component(), path),
        move |ui: &mut BevyUi| {
            let Some(Entry::Variant {
                variants,
                pick,
                children,
                ..
            }) = entries(ui.world, &field).into_iter().next()
            else {
                return;
            };
            ui.compose(enums::VariantPicker {
                source: &field,
                variants,
                pick,
            });
            build_entries(ui, &field, children, depth + 1);
        },
    ));
}

fn build_group(
    ui: &mut BevyUi,
    root: &Field,
    path: String,
    name: String,
    depth: u32,
) {
    let group_field = root.child(&path);

    // Re-walked from `group_field` on every open rather than carried
    // in the closure, so a section reopened many times never clones
    // stale data forward.
    ui.compose(Section::new(
        name,
        (root.entity(), root.component(), path),
        move |ui: &mut BevyUi| {
            let walked = entries(ui.world, &group_field);
            build_entries(ui, &group_field, walked, depth + 1);
        },
    ));
}

/// A collapsible section: a header that folds it, and a body indented
/// under a guide rail.
pub struct Section<F, H> {
    pub name: String,
    pub body: F,
    /// This section's place in `ClosedSections`, as entity,
    /// component, path.
    pub section: (Entity, TypeId, String),
    /// Run on the header once it's built, after folding is wired to
    /// it - for whatever else the header should carry, like a delete
    /// button. A no-op when left out.
    pub on_header: H,
}

/// [`Section::new`]'s `on_header`: nothing extra on it.
fn no_header(_: ElementMut<'_, '_, FynixHost, Button>) {}

impl<F> Section<F, fn(ElementMut<'_, '_, FynixHost, Button>)> {
    /// A section with nothing extra on its header.
    pub fn new(
        name: String,
        section: (Entity, TypeId, String),
        body: F,
    ) -> Self {
        Self {
            name,
            body,
            section,
            on_header: no_header,
        }
    }
}

impl<
    F: BuildFn<FynixHost>,
    H: for<'u, 'a> FnOnce(ElementMut<'u, 'a, FynixHost, Button>)
        + Send
        + Sync
        + 'static,
> Composer<FynixHost> for Section<F, H>
{
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let Self {
            name,
            body,
            section,
            on_header,
        } = self;
        let (entity, component, path) = section;
        let open = section_open(ui.world, entity, component, &path);

        let muted = ui.theme.color.text_dim;
        let primary = ui.theme.color.text;
        ui.compose(Foldable {
            header: elem!(
                !TintButton::default(),
                width = percent(100),
                height = auto(),
                justify = JustifyContent::FlexStart,
                padding = UiRect::axes(px(4), px(3)),
                radius = px(4),
                icon = elem!(
                    Icon,
                    image = icons::CHEVRON,
                    color = muted,
                    rotation = CHEVRON_SHUT
                ),
                label = elem!(
                    Label,
                    text = name,
                    color = primary,
                    bold = true
                )
            ),
            // Nothing else to mean: the whole header folds it.
            folds_on: FoldsOn::Header,
            enabled: true,
            on_header,
            body,
            open,
            on_toggle: move |world: &mut World, open: bool| {
                toggle_section(
                    world,
                    entity,
                    component,
                    path.clone(),
                    open,
                );
            },
        })
        .handle()
    }
}
