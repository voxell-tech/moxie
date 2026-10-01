//! Inspectors, as views.
//!
//! What an inspector is handed (an entity, a component's type, a
//! resource's type) decides what its subtree *is*, and it follows
//! that as the world changes: each is empty while what it points at
//! is not there, and builds its rows when it appears. A missing
//! component and an inspector pointed nowhere read the same.

use std::any::TypeId;
use std::borrow::Cow;

use bevy::asset::{AssetServer, UntypedAssetId};
use bevy::ecs::reflect::ReflectComponent;
use bevy::prelude::*;
use bevy::reflect::TypeRegistration;
use bevy::reflect::std_traits::ReflectDefault;
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt as _, ContextMenuExt as _, FrameProps as _, Open,
    button, column, frame, icon, label, menu_item, row, tint,
};
use bevy_fynix::{
    AnyView, Bevy, ScopedExt as _, View, ViewExt as _, component,
    each, keyed,
};

use crate::fold::{CHEVRON_OPEN, CHEVRON_SHUT};
use crate::gaps::{anchored, changing_under, placeholder_dropdown};
use crate::icons;
use crate::inspector::{
    Binding, Field, FieldAnimatable, ReflectEssential,
    ReflectInspectGroup, ReflectInspectable, draggable_field,
    field_name, field_row, inspect_value, inspector_fields,
    root_leaf, section_open, toggle_section,
};
use crate::theme::EditorTheme;

type Item = AnyView<Bevy, EditorTheme>;

/// What separates the rows of one component's fields, and the rows
/// of a card's body.
const FIELD_GAP: f32 = 4.0;

/// What separates whole components from each other.
const CARD_GAP: f32 = 8.0;

/// A column of `FIELD_GAP`-spaced rows, filling its parent.
fn rows(gap: f32) -> impl View<Bevy, EditorTheme> {
    frame()
        .direction(FlexDirection::Column)
        .width(percent(100.0))
        .gap(gap)
}

/// Inspector for a [`Component`] of `entity`, named by its type.
///
/// `depth` is how many [`Foldable`](crate::fold::Foldable) bodies it
/// sits under, for its rows to keep their columns aligned. `0` for a
/// call site with none of its own.
pub fn component_inspector(
    entity: Entity,
    component: TypeId,
    depth: u32,
) -> impl View<Bevy, EditorTheme> {
    root_inspector(Field::new(entity, component), depth)
}

/// As [`component_inspector`], for a call site that has the type
/// rather than a [`TypeId`], at depth `0`.
pub fn component_inspector_of<T: Component + Reflect>(
    entity: Entity,
) -> impl View<Bevy, EditorTheme> {
    component_inspector(entity, TypeId::of::<T>(), 0)
}

/// Inspector for a [`Field`]'s whole root: a component or an asset.
/// `depth` is as for [`component_inspector`].
pub fn root_inspector(
    root: Field,
    depth: u32,
) -> impl View<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        let present = root.clone();
        keyed::<EditorTheme, bool>(
            changing_under(anchor, move |world: &World| {
                present.exists(world)
            }),
            move |exists| {
                if *exists {
                    inspector_fields(root.clone(), depth).boxed()
                } else {
                    frame().boxed()
                }
            },
        )
        .within(rows(FIELD_GAP))
    })
}

/// Inspector for a [`Resource`], named by its type.
pub fn resource_inspector(
    resource: TypeId,
) -> impl View<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        keyed::<EditorTheme, Option<Entity>>(
            changing_under(anchor, move |world: &World| {
                resource_entity(world, resource)
            }),
            move |entity| match entity {
                Some(entity) => {
                    inspector_fields(Field::new(*entity, resource), 0)
                        .boxed()
                }
                None => frame().boxed(),
            },
        )
        .within(rows(FIELD_GAP))
    })
}

/// As [`resource_inspector`], for a call site that has the type
/// rather than a [`TypeId`].
pub fn resource_inspector_of<T: Resource + Reflect>()
-> impl View<Bevy, EditorTheme> {
    resource_inspector(TypeId::of::<T>())
}

/// What a component's section lists, in order.
type Inspectable = (TypeId, Cow<'static, str>, Option<&'static str>);

/// One row of an entity's inspector.
#[derive(Clone, PartialEq)]
enum Section {
    /// A group's own name, heading the run of cards under it.
    Heading(&'static str),
    /// One component's card.
    Card { component: TypeId, name: String },
}

/// Inspector for all of the [`Component`]s on an [`Entity`]: a card
/// each, and a menu to add another.
///
/// A component added or removed builds or drops its own card and
/// leaves the others alone.
pub fn entity_inspector(
    entity: Entity,
) -> impl View<Bevy, EditorTheme> {
    column((
        anchored::<EditorTheme, _>(move |anchor| {
            each::<EditorTheme, Section, Section>(
                changing_under(anchor, move |world: &World| {
                    sections(world, entity)
                }),
                |section| section.clone(),
                move |section| match section {
                    Section::Heading(name) => group_heading(name),
                    Section::Card { component, name } => {
                        component_card(
                            entity,
                            *component,
                            name.clone(),
                        )
                    }
                },
            )
            .within(rows(CARD_GAP))
        }),
        add_component_menu(entity),
    ))
    .width(percent(100.0))
    .gap(CARD_GAP)
}

/// The cards `entity` shows, each group's under a heading.
fn sections(world: &World, entity: Entity) -> Vec<Section> {
    // `None` sorts first, so an ungrouped run never gets mistaken
    // for one under its own (absent) heading.
    let mut shown_group: Option<Option<&'static str>> = None;
    let mut out = Vec::new();
    for (component, name, group) in inspectable(world, entity) {
        if shown_group != Some(group) {
            shown_group = Some(group);
            if let Some(group) = group {
                out.push(Section::Heading(group));
            }
        }
        out.push(Section::Card {
            component,
            name: name.to_string(),
        });
    }
    out
}

/// A group's own name, heading the run of cards under it.
fn group_heading(name: &'static str) -> Item {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let theme = cx.theme();
        let (fill, radius, small) = (
            theme.color.fill,
            theme.space.menu_item_radius,
            theme.text.small,
        );
        cx.build(
            column((label(name)
                .size(small)
                .bold(true)
                .wrap(false)
                .tone(Tone::Dim),))
            .width(percent(100.0))
            .padding(UiRect::axes(px(8.0), px(4.0)))
            .fill(fill)
            .radius(radius),
        )
    })
}

/// The menu that adds a component to `entity`: a dropdown whose
/// first row only says what it is for, and whose list is rebuilt as
/// components come and go.
fn add_component_menu(
    entity: Entity,
) -> impl View<Bevy, EditorTheme> {
    anchored::<EditorTheme, _>(move |anchor| {
        keyed::<EditorTheme, Vec<Inspectable>>(
            changing_under(anchor, move |world: &World| {
                addable(world, entity)
            }),
            move |options| {
                add_component_dropdown(entity, options.clone())
            },
        )
    })
}

/// A dropdown of `options`, each a component to add.
fn add_component_dropdown(
    entity: Entity,
    options: Vec<Inspectable>,
) -> Item {
    // A menu popup only opens with a focusable child, so an empty
    // list says why it's empty instead of showing nothing.
    let prompt = if options.is_empty() {
        "Nothing left to add"
    } else {
        "Add component"
    };
    let names = options.iter().map(|(_, name, group)| match group {
        Some(group) => format!("{group} / {name}"),
        None => name.to_string(),
    });
    let components: Vec<TypeId> =
        options.iter().map(|(component, ..)| *component).collect();

    placeholder_dropdown(prompt, names, move |world, at| {
        if let Some(component) = components.get(at) {
            add_component(world, entity, *component);
        }
    })
    .min_width(px(160.0))
    .max_width(px(240.0))
    .boxed()
}

/// Every [`register_inspectable`](
/// crate::inspector::InspectAppExt::register_inspectable) type
/// `entity` does not already carry, with a registered
/// [`ReflectDefault`] to construct one with.
///
/// Sorted by [`with_inspect_group`](
/// crate::inspector::InspectAppExt::with_inspect_group)'s group
/// (ungrouped first), then by name within it, for the same reason
/// [`inspectable`] sorts by name.
fn addable(world: &World, entity: Entity) -> Vec<Inspectable> {
    let Ok(entity_ref) = world.get_entity(entity) else {
        return Vec::new();
    };

    let registry = world.resource::<AppTypeRegistry>().read();
    let mut out: Vec<Inspectable> = registry
        .iter()
        .filter(|registration| {
            registration.data::<ReflectInspectable>().is_some()
                && registration.data::<ReflectDefault>().is_some()
        })
        .filter_map(|registration| {
            let reflect_component =
                registration.data::<ReflectComponent>()?;
            if reflect_component.contains(entity_ref) {
                return None;
            }
            let group = registration
                .data::<ReflectInspectGroup>()
                .map(|group| group.0);
            Some((
                registration.type_id(),
                display_name(registration),
                group,
            ))
        })
        .collect();

    out.sort_by(|(_, a_name, a_group), (_, b_name, b_group)| {
        a_group.cmp(b_group).then_with(|| a_name.cmp(b_name))
    });
    out
}

/// Inserts `component`'s default value onto `entity`. Does nothing
/// if the entity despawned or the component arrived some other way
/// before this runs.
fn add_component(
    world: &mut World,
    entity: Entity,
    component: TypeId,
) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let Some(registration) = registry.get(component) else {
        return;
    };
    let (Some(default), Some(reflect_component)) = (
        registration.data::<ReflectDefault>(),
        registration.data::<ReflectComponent>(),
    ) else {
        return;
    };
    let Ok(mut entity) = world.get_entity_mut(entity) else {
        return;
    };
    if reflect_component.contains(entity.as_readonly()) {
        return;
    }

    let value = default.default();
    reflect_component.insert(
        &mut entity,
        value.as_partial_reflect(),
        &registry,
    );
}

/// Removes `component` from `entity`. Does nothing if the entity
/// despawned, never carried it, or its type isn't registered.
fn remove_component(
    world: &mut World,
    entity: Entity,
    component: TypeId,
) {
    if essential(world, component) {
        return;
    }

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let Some(reflect_component) =
        registry.get(component).and_then(|registration| {
            registration.data::<ReflectComponent>()
        })
    else {
        return;
    };
    let Ok(mut entity) = world.get_entity_mut(entity) else {
        return;
    };
    reflect_component.remove(&mut entity);
}

/// Whether `component` was opted in via
/// [`register_essential`](crate::inspector::InspectAppExt::register_essential).
fn essential(world: &World, component: TypeId) -> bool {
    let registry = world.resource::<AppTypeRegistry>().read();
    registry.get(component).is_some_and(|registration| {
        registration.data::<ReflectEssential>().is_some()
    })
}

/// One asset's own card, the same as a component's, titled `title`.
/// With `rename`, a Name row above its fields edits what the asset is
/// called.
pub fn asset_card(
    id: UntypedAssetId,
    title: String,
    rename: Option<Binding>,
) -> impl View<Bevy, EditorTheme> {
    root_card(Field::asset(id), title, rename)
}

/// One component's own card.
fn component_card(
    entity: Entity,
    component: TypeId,
    name: String,
) -> Item {
    root_card(Field::new(entity, component), name, None).boxed()
}

/// One root's own card: a title that's always there, above a body
/// that folds flush under it - no rail, no indent, each field reading
/// like its own root - like Unity's per-component panel.
///
/// Whether it is open is an [`Open`] on the card's own node, and is
/// kept besides in the same store the nested sections use, so it
/// survives the card being built again.
fn root_card(
    root: Field,
    name: String,
    rename: Option<Binding>,
) -> impl View<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let (owner, root_type) = (root.owner(), root.root_type());
        // Only a component can be taken off what holds it.
        let deletable =
            root.entity().filter(|_| !essential(cx.world, root_type));
        let open = section_open(cx.world, owner, root_type, "");
        // The title stands in for a genuine field's own name when
        // the whole root is one nameless leaf, so it carries that
        // field's drag source too, same as `field_name` gives a row
        // of its own.
        let drag_field = root_leaf(cx.world, &root).filter(|field| {
            cx.world
                .resource::<FieldAnimatable>()
                .allows(cx.world, field)
        });
        let assets = cx.world.resource::<AssetServer>();
        let chevron = assets.load(icons::CHEVRON);
        let trash: Handle<Image> = assets.load(icons::TRASH);
        let theme = cx.theme();
        let space = theme.space;
        let panel = theme.color.panel;

        let card = cx.build(
            frame()
                .direction(FlexDirection::Column)
                .width(percent(100.0))
                .gap(0.0)
                .fill(panel)
                .radius(space.card_radius)
                .padding(UiRect::all(px(space.card_padding)))
                .overflow(Overflow::clip()),
        );
        if open {
            cx.world.entity_mut(card).insert(Open);
        }

        let chevron = icon(chevron)
            .tone(Tone::Dim)
            .size(space.icon)
            .rotation(component::<Open, _>(card, |open| {
                if open.is_some() {
                    CHEVRON_OPEN
                } else {
                    CHEVRON_SHUT
                }
            }))
            .transition(Motion::Interact);
        let title = name.clone();
        let mut header = button(
            row((chevron, label(title).bold(true).wrap(false)))
                .align(AlignItems::Center)
                .gap(space.sm),
        )
        .width(percent(100.0))
        .justify(JustifyContent::FlexStart)
        .rules(tint)
        .on_activate(move |world| {
            let opening = world.get::<Open>(card).is_none();
            if let Ok(mut card) = world.get_entity_mut(card) {
                if opening {
                    card.insert(Open);
                } else {
                    card.remove::<Open>();
                }
            }
            toggle_section(
                world,
                owner,
                root_type,
                String::new(),
                opening,
            );
        })
        .boxed();

        if let Some(field) = drag_field {
            header = draggable_field(header, field, name).boxed();
        }
        if let Some(entity) = deletable {
            header = header
                .context_menu(move || {
                    (menu_item(
                        row((
                            icon(trash.clone()),
                            label("Delete").wrap(false),
                        ))
                        .align(AlignItems::Center)
                        .gap(space.md)
                        .toned(Tone::Critical),
                    )
                    .on_activate(move |world| {
                        remove_component(world, entity, root_type);
                    }),)
                })
                .boxed();
        }

        // Built while open and dropped while shut, as a fold's body.
        let body = keyed::<EditorTheme, bool>(
            changing_under(Some(card), move |world: &World| {
                world.get::<Open>(card).is_some()
            }),
            move |open| {
                if *open {
                    card_body(root.clone(), rename.clone())
                } else {
                    frame().boxed()
                }
            },
        )
        .within(
            frame()
                .direction(FlexDirection::Column)
                .width(percent(100.0))
                .gap(0.0),
        );
        cx.under(card, |cx| {
            cx.build(header);
            cx.build(body);
        });
        card
    })
}

/// What a card holds while open: the optional Name row, then the
/// root's fields.
fn card_body(root: Field, rename: Option<Binding>) -> Item {
    let mut rows: Vec<Item> = Vec::new();
    if let Some(rename) = rename {
        rows.push(
            field_row(
                Some(
                    field_name(None, "Name").tone(Tone::Dim).boxed(),
                ),
                inspect_value(rename),
                0,
            )
            .boxed(),
        );
    }
    rows.push(root_inspector(root, 0).boxed());
    column(rows).width(percent(100.0)).gap(0.0).boxed()
}

/// The entity bevy is currently keeping `resource` on.
fn resource_entity(
    world: &World,
    resource: TypeId,
) -> Option<Entity> {
    let id = world.components().get_id(resource)?;
    world.resource_entities().get(id)
}

/// Every component on `entity` the inspector can reach and shows, by
/// type, the name its section is headed with, and its
/// [`with_inspect_group`](
/// crate::inspector::InspectAppExt::with_inspect_group) group.
///
/// Sorted by group (ungrouped first), then by name within it: an
/// archetype lists what it holds in whatever order it happens to, and
/// a panel whose sections reshuffle when a component is added is no
/// use to read.
fn inspectable(world: &World, entity: Entity) -> Vec<Inspectable> {
    let Ok(components) = world.inspect_entity(entity) else {
        return Vec::new();
    };
    // Collected before the registry is read: both borrow the world.
    let ids: Vec<TypeId> =
        components.filter_map(|info| info.type_id()).collect();

    let registry = world.resource::<AppTypeRegistry>().read();
    let mut out: Vec<Inspectable> = ids
        .into_iter()
        .filter_map(|id| {
            let registration = registry.get(id)?;
            // Without this there is no way to reach the value at
            // all, whatever its fields would have said.
            registration.data::<ReflectComponent>()?;
            // Opt-in - see InspectAppExt::register_inspectable.
            registration.data::<ReflectInspectable>()?;
            let group = registration
                .data::<ReflectInspectGroup>()
                .map(|group| group.0);
            Some((id, display_name(registration), group))
        })
        .collect();

    out.sort_by(|(_, a_name, a_group), (_, b_name, b_group)| {
        a_group.cmp(b_group).then_with(|| a_name.cmp(b_name))
    });
    out
}

/// What [`ReflectInspectable::name`] overrides to, or `T`'s own name
/// split into words.
pub fn display_name(
    registration: &TypeRegistration,
) -> Cow<'static, str> {
    if let Some(name) = registration
        .data::<ReflectInspectable>()
        .and_then(|inspectable| inspectable.name)
    {
        return Cow::Borrowed(name);
    }

    Cow::Owned(humanize(
        registration.type_info().type_path_table().short_path(),
    ))
}

/// `name` split at each lowercase-to-uppercase or letter-to-digit
/// boundary, so a type's own Rust-cased name reads as words.
fn humanize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev: Option<char> = None;

    for ch in name.chars() {
        let splits = prev.is_some_and(|prev| {
            (prev.is_lowercase() && ch.is_uppercase())
                || (prev.is_alphabetic() && ch.is_numeric())
        });
        if splits {
            out.push(' ');
        }
        out.push(ch);
        prev = Some(ch);
    }

    out
}

#[cfg(test)]
mod tests {
    use bevy::text::EditableText;
    use bevy::ui_widgets::{Activate, Button, MenuItem};

    use super::*;
    use crate::inspector::InspectAppExt as _;
    use crate::tests::{self, Probe};

    /// A resource with one field to edit.
    #[derive(Resource, Reflect, Default)]
    #[reflect(Resource, Default)]
    struct Gain {
        amount: f32,
    }

    fn inputs(app: &App, root: Entity) -> Vec<String> {
        tests::inputs(app, root)
    }

    fn labels(app: &App, root: Entity) -> Vec<String> {
        tests::all::<Text>(app, root)
            .into_iter()
            .map(|node| {
                app.world().get::<Text>(node).unwrap().0.clone()
            })
            .collect()
    }

    #[test]
    fn a_component_inspector_follows_its_component_in_and_out() {
        let (mut app, probe) = tests::probe_app();
        let root = tests::show(
            &mut app,
            component_inspector_of::<Probe>(probe),
        );
        assert_eq!(inputs(&app, root).len(), 10);

        app.world_mut().entity_mut(probe).remove::<Probe>();
        app.update();
        assert!(inputs(&app, root).is_empty());

        app.world_mut().entity_mut(probe).insert(Probe {
            level: 2.0,
            ..Probe::default()
        });
        app.update();
        assert_eq!(inputs(&app, root).len(), 10);
        assert!(inputs(&app, root).contains(&"2".to_string()));
    }

    #[test]
    fn a_resource_inspector_follows_its_resource_in_and_out() {
        let mut app = tests::app();
        app.register_type::<Gain>()
            .insert_resource(Gain { amount: 3.0 });
        let root =
            tests::show(&mut app, resource_inspector_of::<Gain>());
        assert_eq!(inputs(&app, root), ["3"]);

        app.world_mut().remove_resource::<Gain>();
        app.update();
        assert!(inputs(&app, root).is_empty());

        app.insert_resource(Gain { amount: 5.0 });
        app.update();
        assert_eq!(inputs(&app, root), ["5"]);
    }

    fn cube(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((Name::new("cube"), Transform::default()))
            .id()
    }

    #[test]
    fn an_entity_inspector_has_a_card_per_inspectable_component() {
        let mut app = tests::app();
        let entity = cube(&mut app);
        let root = tests::show(&mut app, entity_inspector(entity));

        let names = labels(&app, root);
        assert!(names.contains(&"Name".to_string()), "{names:?}");
        assert!(names.contains(&"Transform".to_string()));
        assert!(
            !names.contains(&"Global Transform".to_string()),
            "reflected, never opted in"
        );
        assert!(inputs(&app, root).contains(&"cube".to_string()));
    }

    #[test]
    fn a_component_added_builds_its_card_and_leaves_the_others() {
        let mut app = tests::app();
        let entity = cube(&mut app);
        let root = tests::show(&mut app, entity_inspector(entity));
        let before = tests::below(&app, root);

        app.world_mut()
            .entity_mut(entity)
            .insert(Visibility::Hidden);
        app.update();

        assert!(
            labels(&app, root).contains(&"Visibility".to_string())
        );
        let after = tests::below(&app, root);
        // The add menu rebuilds as its list shortens, so the cards
        // are what has to be there still.
        assert!(
            before
                .iter()
                .filter(|node| {
                    app.world().get::<Open>(**node).is_some()
                })
                .all(|card| after.contains(card)),
            "the cards there before are still there"
        );
    }

    #[test]
    fn the_add_menu_lists_what_is_left_and_adds_what_is_picked() {
        let mut app = tests::app();
        let entity = cube(&mut app);
        let root = tests::show(&mut app, entity_inspector(entity));

        let options = labels(&app, root);
        assert!(options.contains(&"Add component".to_string()));
        assert!(
            options.contains(&"Cameras / Camera 2d".to_string()),
            "{options:?}"
        );

        let item = tests::all::<MenuItem>(&app, root)
            .into_iter()
            .find(|item| {
                tests::all::<Text>(&app, *item).iter().any(|text| {
                    app.world().get::<Text>(*text).unwrap().0
                        == "Cameras / Camera 2d"
                })
            })
            .expect("a row for Camera 2d");
        app.world_mut().trigger(Activate { entity: item });
        app.update();

        assert!(app.world().get::<Camera2d>(entity).is_some());
        assert!(
            !labels(&app, root)
                .contains(&"Cameras / Camera 2d".to_string())
        );
        assert!(
            labels(&app, root).contains(&"Camera 2d".to_string())
        );
    }

    #[test]
    fn a_card_folds_its_body_and_remembers_it() {
        let (mut app, probe) = tests::probe_app();
        let root = tests::show(
            &mut app,
            root_card(
                Field::of::<Probe>(probe),
                "Probe".to_string(),
                None,
            ),
        );
        assert!(app.world().get::<Open>(root).is_some());
        assert_eq!(inputs(&app, root).len(), 10);
        let header = tests::all::<Button>(&app, root)[0];

        app.world_mut().trigger(Activate { entity: header });
        app.update();

        assert!(app.world().get::<Open>(root).is_none());
        assert!(inputs(&app, root).is_empty());
        assert!(!section_open(
            app.world(),
            crate::inspector::Owner::Entity(probe),
            TypeId::of::<Probe>(),
            ""
        ));

        app.world_mut().trigger(Activate { entity: header });
        app.update();
        assert_eq!(inputs(&app, root).len(), 10);
    }

    #[test]
    fn a_card_built_again_comes_back_shut() {
        let (mut app, probe) = tests::probe_app();
        toggle_section(
            app.world_mut(),
            crate::inspector::Owner::Entity(probe),
            TypeId::of::<Probe>(),
            String::new(),
            false,
        );
        let root = tests::show(
            &mut app,
            root_card(
                Field::of::<Probe>(probe),
                "Probe".to_string(),
                None,
            ),
        );

        assert!(app.world().get::<Open>(root).is_none());
        assert!(inputs(&app, root).is_empty());
    }

    #[test]
    fn a_rename_row_edits_what_the_card_is_called() {
        let (mut app, probe) = tests::probe_app();
        app.world_mut().get_mut::<Probe>(probe).unwrap().name =
            "ada".into();
        let rename =
            Binding::from(Field::of::<Probe>(probe).child("name"));
        let root = tests::show(
            &mut app,
            root_card(
                Field::of::<Probe>(probe),
                "Probe".to_string(),
                Some(rename),
            ),
        );

        assert_eq!(labels(&app, root)[1], "Name");
        assert_eq!(inputs(&app, root)[0], "ada");
        assert!(
            tests::all::<EditableText>(&app, root).len() == 11,
            "the rename input comes first, then the ten fields"
        );
    }

    #[test]
    fn an_essential_component_cannot_be_removed() {
        let mut app = tests::app();
        let entity = app
            .world_mut()
            .spawn((Transform::default(), Camera2d))
            .id();

        remove_component(
            app.world_mut(),
            entity,
            TypeId::of::<Transform>(),
        );
        remove_component(
            app.world_mut(),
            entity,
            TypeId::of::<Camera2d>(),
        );

        assert!(app.world().get::<Transform>(entity).is_some());
        assert!(app.world().get::<Camera2d>(entity).is_none());
    }

    #[test]
    fn a_section_is_named_by_its_type_unless_it_says_otherwise() {
        let mut app = tests::app();
        app.register_inspectable::<Probe>();
        let registry =
            app.world().resource::<AppTypeRegistry>().read();
        let name = |id| display_name(registry.get(id).unwrap());

        assert_eq!(name(TypeId::of::<Probe>()), "Probe");
        assert_eq!(
            name(TypeId::of::<MeshMaterial3d<StandardMaterial>>()),
            "PBR Material"
        );
        assert_eq!(name(TypeId::of::<Camera2d>()), "Camera 2d");
        assert_eq!(humanize("DirectionalLight"), "Directional Light");
    }
}
