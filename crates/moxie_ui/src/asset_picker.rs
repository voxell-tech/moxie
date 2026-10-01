//! Picking the asset a [`Handle<T>`] field holds, from a window of
//! its own: a searchable grid of every [`asset_choices`] entry for
//! `T`, each with a thumbnail when the app registered a way to render
//! one.
//!
//! A click assigns at once, so the scene shows the pick while the
//! window is still open. A double-click or Enter keeps it and closes,
//! Escape puts back what the field held before it opened.

use core::any::TypeId;
use std::collections::HashMap;

use bevy::asset::Asset;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::events::{Click, Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui::OverrideClip;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, MENU_Z, TooltipExt as _,
    button, column, frame, ghost, icon, label, menu_surface, overlay,
    row, scroll, text_field,
};
use bevy_fynix::{
    AnyView, Bevy, Cx, ScopedExt as _, View, ViewExt as _, component,
    keyed, mount,
};
use moxie_asset::{AssetRef, AssetType, AssetTypes, asset_choices};

use crate::gaps::{at_point, changing_under, tint_to, wrapping};
use crate::icons;
use crate::inspector::Binding;
use crate::theme::EditorTheme;

const WIDTH: f32 = 372.0;
const HEIGHT: f32 = 440.0;
const THUMBNAIL: f32 = 64.0;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Thumbnails>()
        .add_systems(Update, picker_keys);
}

/// Every thumbnail rendered so far.
#[derive(Resource, Default)]
struct Thumbnails(HashMap<(TypeId, AssetRef), Handle<Image>>);

/// What the app registered about `kind`.
fn asset_type(world: &World, kind: TypeId) -> Option<&AssetType> {
    world.get_resource::<AssetTypes>()?.get(kind)
}

/// The thumbnail for the `kind` asset `asset`, rendered on first ask
/// and reused after.
fn thumbnail(
    world: &mut World,
    kind: TypeId,
    asset: &AssetRef,
) -> Option<Handle<Image>> {
    let key = (kind, asset.clone());
    if let Some(image) = world.resource::<Thumbnails>().0.get(&key) {
        return Some(image.clone());
    }
    let render = asset_type(world, kind)?.thumbnail?;

    let image = render(world, asset)?;
    world
        .resource_mut::<Thumbnails>()
        .0
        .insert(key, image.clone());
    Some(image)
}

/// Drops every internal asset's thumbnail, which may have been edited
/// since it was rendered.
fn forget_internal_thumbnails(world: &mut World) {
    world
        .resource_mut::<Thumbnails>()
        .0
        .retain(|(_, asset), _| !matches!(asset, AssetRef::Uuid(_)));
}

/// The open picker's own root. There is at most one.
#[derive(Component)]
pub(crate) struct AssetPickerRoot;

/// What the search box holds.
#[derive(Component, Default)]
struct Search(String);

/// Puts back what the field held when the picker opened, and closes
/// it.
#[derive(EntityEvent)]
struct Cancel {
    entity: Entity,
}

/// Asks the app to bring [`moxie_asset::FoundAssets`] up to date,
/// before a picker lists them.
#[derive(Event)]
pub struct RefreshAssetChoices;

/// One choice of the grid. `None` clears the field.
#[derive(Clone, PartialEq)]
struct Cell {
    name: String,
    asset: Option<AssetRef>,
    group: String,
}

/// What the grid lists: a choice, or the name of the group the
/// choices after it belong to.
#[derive(Clone, PartialEq)]
enum Entry {
    Heading(String),
    Cell(Cell),
}

/// The entries the search lets through: "None", unless `T` is
/// [required](AssetType::required), then every [`asset_choices`]
/// entry for `T`, each run of a group under its heading.
fn entries<T: Asset>(world: &World, root: Entity) -> Vec<Entry> {
    let query = world
        .get::<Search>(root)
        .map(|search| search.0.trim().to_lowercase())
        .unwrap_or_default();
    let required = asset_type(world, TypeId::of::<T>())
        .is_some_and(|info| info.required);

    let none = (!required).then(|| Cell {
        name: "None".to_string(),
        asset: None,
        group: String::new(),
    });
    let choices = asset_choices::<T>(world).map(|choice| Cell {
        name: choice.name.clone(),
        asset: Some(choice.asset.clone()),
        group: choice.group.clone(),
    });

    let mut entries = Vec::new();
    let mut group = String::new();
    for cell in none.into_iter().chain(choices) {
        if !cell.name.to_lowercase().contains(&query) {
            continue;
        }
        if cell.group != group {
            group.clone_from(&cell.group);
            entries.push(Entry::Heading(group.clone()));
        }
        entries.push(Entry::Cell(cell));
    }
    entries
}

/// Opens the picker for the `T` that `binding` holds, anchored at
/// `at` in logical screen space, closing any picker already open.
pub(crate) fn open_asset_picker<T: Asset + TypePath>(
    world: &mut World,
    at: Vec2,
    binding: Binding,
) {
    close_asset_picker(world);
    forget_internal_thumbnails(world);
    world.trigger(RefreshAssetChoices);

    let original = binding.read::<Handle<T>>(world);
    let title = format!("Select {}", T::short_type_path());
    mount::<EditorTheme>(
        world,
        window::<T>(at, title, binding, original),
    );
}

fn close_asset_picker(world: &mut World) {
    let open = world
        .query_filtered::<Entity, With<AssetPickerRoot>>()
        .iter(world)
        .collect::<Vec<_>>();
    for root in open {
        despawn(world, root);
    }
}

fn despawn(world: &mut World, root: Entity) {
    if let Ok(entity) = world.get_entity_mut(root) {
        entity.despawn();
    }
}

fn picker_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    roots: Query<Entity, With<AssetPickerRoot>>,
    mut commands: Commands,
) {
    let (Some(keys), Ok(root)) = (keys, roots.single()) else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        commands.trigger(Cancel { entity: root });
    } else if keys.just_pressed(KeyCode::Enter) {
        commands.entity(root).despawn();
    }
}

/// What `binding` holds, if it can be named at all.
fn current<T: Asset + TypePath>(
    world: &World,
    binding: &Binding,
) -> Option<AssetRef> {
    let handle = binding.read::<Handle<T>>(world)?;
    AssetRef::of(&handle, world.get_resource::<AssetServer>()?)
}

/// The picker's whole view: a backdrop that closes it, and under it
/// the window, hung off the point it was opened at.
fn window<T: Asset + TypePath>(
    at: Vec2,
    title: String,
    binding: Binding,
    original: Option<Handle<T>>,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let space = cx.theme().space;
        let root = cx.build(overlay(()));

        let revert = binding.clone();
        cx.world
            .entity_mut(root)
            .insert((AssetPickerRoot, Search::default()))
            .observe(
                move |cancel: On<Cancel>, mut commands: Commands| {
                    let (binding, original, root) = (
                        revert.clone(),
                        original.clone(),
                        cancel.entity,
                    );
                    commands.queue(move |world: &mut World| {
                        if let Some(original) = original {
                            binding.set(world, &original);
                        }
                        despawn(world, root);
                    });
                },
            );

        cx.under(root, |cx| {
            // Closing on a click elsewhere keeps the pick, like the
            // close button does.
            let backdrop = cx.build(
                frame()
                    .position(PositionType::Absolute)
                    .inset(UiRect::all(px(0.0)))
                    .width(percent(100.0))
                    .height(percent(100.0))
                    .z(Some(MENU_Z - 1)),
            );
            cx.world.entity_mut(backdrop).observe(
                move |_: On<Pointer<Press>>,
                      mut commands: Commands| {
                    commands.queue(move |world: &mut World| {
                        despawn(world, root);
                    });
                },
            );

            let anchor = cx.build(
                row(()).position(PositionType::Absolute).inset(
                    UiRect::new(
                        px(at.x),
                        Val::Auto,
                        px(at.y),
                        Val::Auto,
                    ),
                ),
            );
            cx.under(anchor, |cx| {
                cx.build(
                    column((
                        header::<T>(root, title, binding.clone()),
                        search(root),
                        grid::<T>(root, binding.clone()),
                        footer::<T>(binding),
                    ))
                    .width(px(WIDTH))
                    .height(px(HEIGHT))
                    .gap(space.md)
                    .padding(UiRect::all(px(space.md)))
                    .with((at_point(space.menu_margin), OverrideClip))
                    .rules(
                        |cx: &mut Cx<'_, Bevy, EditorTheme>| {
                            cx.defaults(menu_surface);
                        },
                    ),
                )
            });
        });
        root
    })
}

/// The title, a "New" button when `T` can be made, and the close
/// button.
fn header<T: Asset + TypePath>(
    root: Entity,
    title: String,
    binding: Binding,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let create = asset_type(cx.world, TypeId::of::<T>())
            .and_then(|info| info.create);
        let assets = cx.world.resource::<AssetServer>();
        let plus = assets.load::<Image>(icons::PLUS);
        let close = assets.load::<Image>(icons::CLOSE);

        let mut items = vec![
            label(title).bold(true).boxed(),
            frame().grow(1.0).boxed(),
        ];
        if let Some(create) = create {
            items.push(
                button(
                    row((
                        icon(plus).tone(Tone::Dim).size(10.0),
                        label("New"),
                    ))
                    .align(AlignItems::Center)
                    .gap(4.0),
                )
                .rules(ghost)
                .tooltip(|| {
                    label("New, starting from the current one")
                })
                .on_activate(move |world| {
                    let seed = binding
                        .read::<Handle<T>>(world)
                        .map(|handle| handle.id().untyped());
                    let Some(handle) = create(world, seed) else {
                        return;
                    };
                    binding.set(world, &handle.typed::<T>());
                    world.trigger(RefreshAssetChoices);
                })
                .boxed(),
            );
        }
        items.push(
            button(icon(close).size(10.0))
                .width(px(14.0))
                .height(px(14.0))
                .padding(UiRect::ZERO)
                .radius(2.0)
                .rules(tint_to(Tone::Dim, Tone::Critical))
                .on_activate(move |world| despawn(world, root))
                .boxed(),
        );
        cx.build(
            row(items)
                .width(percent(100.0))
                .align(AlignItems::Center)
                .gap(4.0),
        )
    })
}

/// The search box, focused as the picker opens, whose text filters
/// the grid as it is typed.
fn search(root: Entity) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let held = component::<Search, _>(root, |search| {
            search.map(|search| search.0.clone()).unwrap_or_default()
        });
        let field = cx.build(
            text_field(held, move |world, text| {
                set_search(world, root, text);
            })
            .width(percent(100.0)),
        );
        let input = cx
            .world
            .get::<Children>(field)
            .and_then(|kids| kids.first().copied());
        if let Some(input) = input {
            // Also fires on a bare cursor move, which would
            // otherwise rebuild the grid under the pointer.
            cx.world.entity_mut(input).observe(
                move |change: On<TextEditChange>,
                      texts: Query<&EditableText>,
                      mut searches: Query<&mut Search>| {
                    let (Ok(text), Ok(mut search)) = (
                        texts.get(change.event_target()),
                        searches.get_mut(root),
                    ) else {
                        return;
                    };
                    let text = text.value().to_string();
                    if search.0 != text {
                        search.0 = text;
                    }
                },
            );
            cx.world
                .resource_mut::<InputFocus>()
                .set(input, FocusCause::Navigated);
        }
        field
    })
}

fn set_search(world: &mut World, root: Entity, text: String) {
    if let Some(mut search) = world.get_mut::<Search>(root)
        && search.0 != text
    {
        search.0 = text;
    }
}

/// The scrolling grid of what [`entries`] lists, built again
/// whenever that changes.
fn grid<T: Asset + TypePath>(
    root: Entity,
    binding: Binding,
) -> impl View<Bevy, EditorTheme> {
    let listed = changing_under(Some(root), move |world: &World| {
        entries::<T>(world, root)
    });
    let cells =
        keyed::<EditorTheme, Vec<Entry>>(listed, move |list| {
            let (list, binding) = (list.clone(), binding.clone());
            AnyView::<Bevy, EditorTheme>::new(move |cx| {
                let gap = cx.theme().space.sm;
                let views = list
                    .iter()
                    .map(|entry| match entry {
                        Entry::Heading(group) => {
                            heading(group.clone())
                        }
                        Entry::Cell(cell) => cell_view::<T>(
                            cell.clone(),
                            root,
                            binding.clone(),
                        ),
                    })
                    .collect::<Vec<_>>();
                cx.build(wrapping(
                    row(views).width(percent(100.0)).gap(gap),
                ))
            })
        })
        .within(column(()).width(percent(100.0)).gap(0.0));

    scroll((cells,))
        .overflow(Overflow::scroll_y())
        .width(percent(100.0))
        .grow(1.0)
}

/// A group's name, on a line of its own.
fn heading(group: String) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let gap = cx.theme().space.sm;
        cx.build(
            column((label(group).size(11.0).tone(Tone::Dim),))
                .width(percent(100.0))
                .padding(UiRect::top(px(gap))),
        )
    })
}

/// One choice: its thumbnail over its name, tinted while it is what
/// the field holds.
fn cell_view<T: Asset + TypePath>(
    cell: Cell,
    root: Entity,
    binding: Binding,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let thumbnail = cell.asset.as_ref().and_then(|asset| {
            thumbnail(cx.world, TypeId::of::<T>(), asset)
        });
        let theme = cx.theme();
        let (pad, fill, selection) =
            (theme.space.sm, theme.color.fill, theme.color.selection);
        let placeholder = cx
            .world
            .resource::<AssetServer>()
            .load::<Image>(icons::ASSET);

        let highlighted = cell.asset.clone();
        let selected = binding.derive(move |binding, world| {
            if current::<T>(world, binding) == highlighted {
                selection
            } else {
                Color::NONE
            }
        });

        let tile = |inside: Vec<AnyView<Bevy, EditorTheme>>| {
            column(inside)
                .width(px(THUMBNAIL))
                .height(px(THUMBNAIL))
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .gap(0.0)
                .radius(4.0)
                .fill(fill)
        };
        let tile = match thumbnail {
            Some(image) => {
                tile(Vec::new()).with(ImageNode::new(image)).boxed()
            }
            None if cell.asset.is_some() => tile(vec![
                icon(placeholder).size(20.0).tone(Tone::Dim).boxed(),
            ])
            .boxed(),
            None => tile(Vec::new()).boxed(),
        };

        let node = cx.build(
            button(
                column((tile, label(cell.name.clone()).size(11.0)))
                    .width(percent(100.0))
                    .align(AlignItems::Center)
                    .gap(pad),
            )
            .width(px(THUMBNAIL + 2.0 * pad))
            .align(AlignItems::Start)
            .padding(UiRect::all(px(pad)))
            .radius(4.0)
            .overflow(Overflow::clip())
            .fill(selected),
        );

        let assigned = cell.asset.clone();
        cx.world.entity_mut(node).observe(
            move |click: On<Pointer<Click>>,
                  mut commands: Commands| {
                if click.button != PointerButton::Primary {
                    return;
                }
                let double = click.count >= 2;
                let (binding, asset) =
                    (binding.clone(), assigned.clone());
                commands.queue(move |world: &mut World| {
                    let handle = match asset {
                        Some(asset) => asset
                            .handle(world.resource::<AssetServer>()),
                        None => Handle::<T>::default(),
                    };
                    binding.set(world, &handle);
                    if double {
                        despawn(world, root);
                    }
                });
            },
        );
        node
    })
}

/// The pick, by name and where it comes from.
fn footer<T: Asset + TypePath>(
    binding: Binding,
) -> impl View<Bevy, EditorTheme> {
    let described = binding.derive(|binding, world| {
        let Some(asset) = current::<T>(world, binding) else {
            return "None".to_string();
        };
        let name = asset_choices::<T>(world)
            .find(|choice| choice.asset == asset)
            .map(|choice| choice.name.clone());
        // A file also shows where it is. Anything else has only its
        // name to go by.
        match (name, asset) {
            (Some(name), AssetRef::Path(path)) => {
                format!("{name}  {path}")
            }
            (None, AssetRef::Path(path)) => path,
            (Some(name), AssetRef::Uuid(_)) => name,
            (None, AssetRef::Uuid(_)) => "(unnamed)".to_string(),
        }
    });
    label(described).size(11.0).wrap(false).tone(Tone::Dim)
}

#[cfg(test)]
mod tests {
    use bevy::asset::uuid::Uuid;
    use bevy::input::keyboard::Key;
    use bevy::ui::widget::Text;
    use bevy::ui_widgets::{Activate, Button};
    use bevy_fynix::ReducedMotion;
    use moxie_asset::{
        AssetChoice, AssetTypeAppExt as _, FoundAssets,
    };

    use super::*;
    use crate::inspector::Field;
    use crate::tests;

    /// A field holding an image, for the picker to edit.
    #[derive(Component, Reflect, Default)]
    #[reflect(Component, Default)]
    struct Holder {
        image: Handle<Image>,
    }

    fn uuid(id: u128) -> Handle<Image> {
        Handle::from(Uuid::from_u128(id))
    }

    fn choice(name: &str, group: &str, id: u128) -> AssetChoice {
        AssetChoice {
            name: name.to_string(),
            asset: AssetRef::Uuid(Uuid::from_u128(id)),
            group: group.to_string(),
        }
    }

    /// An app listing Logo, then Sky and Sea under "Env", with a
    /// holder of an image and its binding.
    fn setup() -> (App, Entity, Binding) {
        let mut app = tests::app();
        app.register_type::<Holder>()
            .insert_resource(ReducedMotion(true));
        app.world_mut().resource_mut::<FoundAssets>().0.insert(
            TypeId::of::<Image>(),
            vec![
                choice("Logo", "", 1),
                choice("Sky", "Env", 2),
                choice("Sea", "Env", 3),
            ],
        );
        let holder = app.world_mut().spawn(Holder::default()).id();
        let binding =
            Binding::from(Field::of::<Holder>(holder).child("image"));
        (app, holder, binding)
    }

    fn open(app: &mut App, binding: &Binding) -> Entity {
        open_asset_picker::<Image>(
            app.world_mut(),
            Vec2::new(10.0, 20.0),
            binding.clone(),
        );
        app.update();
        picker(app).expect("an open picker")
    }

    fn picker(app: &mut App) -> Option<Entity> {
        app.world_mut()
            .query_filtered::<Entity, With<AssetPickerRoot>>()
            .iter(app.world())
            .next()
    }

    fn held(app: &App, holder: Entity) -> Handle<Image> {
        app.world().get::<Holder>(holder).unwrap().image.clone()
    }

    fn texts(app: &App, node: Entity) -> Vec<String> {
        tests::all::<Text>(app, node)
            .into_iter()
            .map(|node| {
                app.world().get::<Text>(node).unwrap().0.clone()
            })
            .collect()
    }

    /// The button with `text` somewhere in it.
    fn button_with(app: &App, root: Entity, text: &str) -> Entity {
        tests::all::<Button>(app, root)
            .into_iter()
            .find(|node| texts(app, *node).iter().any(|t| t == text))
            .unwrap_or_else(|| panic!("a button with {text}"))
    }

    fn fill(app: &App, node: Entity) -> Color {
        app.world().get::<BackgroundColor>(node).unwrap().0
    }

    /// The thumbnail tiles under `root`, which show an image.
    fn tiles(app: &App, root: Entity) -> Vec<Entity> {
        tests::all::<ImageNode>(app, root)
            .into_iter()
            .filter(|node| {
                app.world().get::<Node>(*node).unwrap().width
                    == px(THUMBNAIL)
            })
            .collect()
    }

    #[test]
    fn the_cells_wrap_onto_lines() {
        let (mut app, _, binding) = setup();
        let root = open(&mut app, &binding);

        let wrapping =
            tests::below(&app, root).into_iter().any(|node| {
                app.world()
                    .get::<Node>(node)
                    .is_some_and(|ui| ui.flex_wrap == FlexWrap::Wrap)
            });
        assert!(wrapping);
    }

    #[test]
    fn the_close_button_turns_critical_under_the_pointer() {
        let (mut app, _, binding) = setup();
        let root = open(&mut app, &binding);
        let close = tests::all::<Button>(&app, root)
            .into_iter()
            .find(|node| texts(&app, *node).is_empty())
            .expect("a close button");
        let image = tests::below(&app, close)[0];
        let theme = EditorTheme::default();
        let tint = |app: &App| {
            app.world().get::<ImageNode>(image).unwrap().color
        };
        assert_eq!(tint(&app), theme.color.text_dim);

        app.world_mut()
            .entity_mut(close)
            .insert(bevy_fynix::Hovered);
        app.update();

        assert_eq!(tint(&app), theme.color.critical);
    }

    #[test]
    fn the_footer_reads_none_while_the_field_cannot_be_read() {
        let (mut app, _, _) = setup();
        let nowhere = app.world_mut().spawn_empty().id();
        let binding = Binding::from(
            Field::of::<Holder>(nowhere).child("image"),
        );
        let root = open(&mut app, &binding);

        assert_eq!(
            texts(&app, root).last().map(String::as_str),
            Some("None")
        );
    }

    #[test]
    fn the_grid_lists_none_then_the_choices_under_their_groups() {
        let (mut app, _, binding) = setup();
        let root = open(&mut app, &binding);

        assert_eq!(
            texts(&app, root),
            [
                "Select Image",
                "None",
                "Logo",
                "Env",
                "Sky",
                "Sea",
                "(unnamed)",
            ],
            "the last is the footer, for the default handle"
        );
    }

    #[test]
    fn a_required_type_offers_no_none() {
        let (mut app, _, binding) = setup();
        app.asset_type::<Image>().required = true;
        let root = open(&mut app, &binding);

        assert_eq!(
            texts(&app, root)[1..4],
            ["Logo", "Env", "Sky"],
            "no None cell after the title"
        );
    }

    #[test]
    fn the_search_takes_focus_and_filters_by_name() {
        let (mut app, _, binding) = setup();
        let root = open(&mut app, &binding);

        let input = tests::all::<EditableText>(&app, root)[0];
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(input)
        );

        app.world_mut().get_mut::<Search>(root).unwrap().0 =
            " SE ".to_string();
        app.update();
        assert_eq!(
            texts(&app, root),
            ["Select Image", "Env", "Sea", "(unnamed)"]
        );
    }

    #[test]
    fn a_click_assigns_at_once_and_marks_the_cell() {
        let (mut app, holder, binding) = setup();
        let root = open(&mut app, &binding);
        let sky = button_with(&app, root, "Sky");
        let logo = button_with(&app, root, "Logo");
        let selection = EditorTheme::default().color.selection;

        tests::click(&mut app, sky, PointerButton::Primary, 1);
        app.update();

        assert_eq!(held(&app, holder), uuid(2));
        assert!(picker(&mut app).is_some(), "still open");
        assert_eq!(fill(&app, sky), selection);
        assert_eq!(fill(&app, logo), Color::NONE);
        assert_eq!(
            texts(&app, root).last().map(String::as_str),
            Some("Sky"),
            "the footer names the pick"
        );
    }

    #[test]
    fn another_button_does_not_assign() {
        let (mut app, holder, binding) = setup();
        let root = open(&mut app, &binding);
        let sky = button_with(&app, root, "Sky");

        tests::click(&mut app, sky, PointerButton::Secondary, 1);

        assert_eq!(held(&app, holder), Handle::default());
    }

    #[test]
    fn a_double_click_keeps_the_pick_and_closes() {
        let (mut app, holder, binding) = setup();
        let root = open(&mut app, &binding);
        let sea = button_with(&app, root, "Sea");

        tests::click(&mut app, sea, PointerButton::Primary, 2);

        assert_eq!(held(&app, holder), uuid(3));
        assert!(picker(&mut app).is_none());
    }

    #[test]
    fn none_clears_the_field() {
        let (mut app, holder, binding) = setup();
        app.world_mut().get_mut::<Holder>(holder).unwrap().image =
            uuid(1);
        let root = open(&mut app, &binding);
        let none = button_with(&app, root, "None");

        tests::click(&mut app, none, PointerButton::Primary, 1);

        assert_eq!(held(&app, holder), Handle::default());
    }

    #[test]
    fn enter_keeps_the_pick_and_closes() {
        let (mut app, holder, binding) = setup();
        let root = open(&mut app, &binding);
        let sky = button_with(&app, root, "Sky");
        tests::click(&mut app, sky, PointerButton::Primary, 1);

        tests::key(&mut app, KeyCode::Enter, Key::Enter);

        assert_eq!(held(&app, holder), uuid(2));
        assert!(picker(&mut app).is_none());
    }

    #[test]
    fn escape_puts_back_what_the_field_held_and_closes() {
        let (mut app, holder, binding) = setup();
        app.world_mut().get_mut::<Holder>(holder).unwrap().image =
            uuid(1);
        let root = open(&mut app, &binding);
        let sky = button_with(&app, root, "Sky");
        tests::click(&mut app, sky, PointerButton::Primary, 1);
        assert_eq!(held(&app, holder), uuid(2));

        tests::key(&mut app, KeyCode::Escape, Key::Escape);

        assert_eq!(held(&app, holder), uuid(1));
        assert!(picker(&mut app).is_none());
    }

    #[test]
    fn a_press_on_the_backdrop_keeps_the_pick_and_closes() {
        let (mut app, holder, binding) = setup();
        let root = open(&mut app, &binding);
        let sky = button_with(&app, root, "Sky");
        tests::click(&mut app, sky, PointerButton::Primary, 1);
        let backdrop = tests::below(&app, root)[0];

        tests::press(&mut app, backdrop);

        assert_eq!(held(&app, holder), uuid(2));
        assert!(picker(&mut app).is_none());
    }

    #[test]
    fn the_close_button_keeps_the_pick_and_closes() {
        let (mut app, holder, binding) = setup();
        let root = open(&mut app, &binding);
        let sky = button_with(&app, root, "Sky");
        tests::click(&mut app, sky, PointerButton::Primary, 1);
        let close = tests::all::<Button>(&app, root)
            .into_iter()
            .find(|node| texts(&app, *node).is_empty())
            .expect("a close button");

        app.world_mut().trigger(Activate { entity: close });
        app.update();

        assert_eq!(held(&app, holder), uuid(2));
        assert!(picker(&mut app).is_none());
    }

    #[test]
    fn only_one_picker_is_open_at_a_time() {
        let (mut app, _, binding) = setup();
        open(&mut app, &binding);
        open(&mut app, &binding);

        let roots = app
            .world_mut()
            .query_filtered::<Entity, With<AssetPickerRoot>>()
            .iter(app.world())
            .count();
        assert_eq!(roots, 1);
    }

    #[test]
    fn new_is_offered_only_for_a_type_that_can_be_made() {
        let (mut app, _, binding) = setup();
        let root = open(&mut app, &binding);
        assert!(!texts(&app, root).contains(&"New".to_string()));

        app.asset_type::<Image>().create =
            Some(|_, _| Some(uuid(9).untyped()));
        let root = open(&mut app, &binding);
        assert!(texts(&app, root).contains(&"New".to_string()));
    }

    #[test]
    fn new_assigns_what_the_type_makes() {
        let (mut app, holder, binding) = setup();
        app.asset_type::<Image>().create =
            Some(|_, _| Some(uuid(9).untyped()));
        let root = open(&mut app, &binding);
        let new = button_with(&app, root, "New");

        app.world_mut().trigger(Activate { entity: new });
        app.update();

        assert_eq!(held(&app, holder), uuid(9));
    }

    #[derive(Resource, Default)]
    struct Renders(u32);

    fn render(
        world: &mut World,
        _: &AssetRef,
    ) -> Option<Handle<Image>> {
        world.resource_mut::<Renders>().0 += 1;
        Some(
            world
                .resource_mut::<Assets<Image>>()
                .add(Image::default()),
        )
    }

    #[test]
    fn a_thumbnail_is_rendered_once_and_shown() {
        let (mut app, _, binding) = setup();
        app.init_resource::<Renders>();
        app.asset_type::<Image>().thumbnail = Some(render);
        let file = AssetRef::Path("a.png".to_string());
        app.world_mut().resource_mut::<FoundAssets>().0.insert(
            TypeId::of::<Image>(),
            vec![AssetChoice {
                name: "File".to_string(),
                asset: file,
                group: String::new(),
            }],
        );
        let root = open(&mut app, &binding);

        assert_eq!(app.world().resource::<Renders>().0, 1);
        let shown = tiles(&app, root);
        assert_eq!(shown.len(), 1);
        let image = app.world().get::<ImageNode>(shown[0]).unwrap();
        assert_ne!(image.image, Handle::default());

        // Filtering out and back builds the cell again.
        app.world_mut().get_mut::<Search>(root).unwrap().0 =
            "zzz".to_string();
        app.update();
        app.world_mut().get_mut::<Search>(root).unwrap().0.clear();
        app.update();
        assert_eq!(tiles(&app, root).len(), 1);
        assert_eq!(app.world().resource::<Renders>().0, 1);
    }

    #[test]
    fn an_internal_assets_thumbnail_is_rendered_again_on_open() {
        let (mut app, _, binding) = setup();
        app.init_resource::<Renders>();
        app.asset_type::<Image>().thumbnail = Some(render);
        open(&mut app, &binding);
        let first = app.world().resource::<Renders>().0;
        assert_eq!(first, 3);

        open(&mut app, &binding);
        assert_eq!(app.world().resource::<Renders>().0, first * 2);
    }
}
