//! Picking the asset a [`Handle<T>`] field holds, from a window of its
//! own: a searchable grid of every [`AssetChoices`] entry for `T`,
//! each with a thumbnail when the app registered a way to render one.
//!
//! A click assigns at once, so the scene shows the pick while the
//! window is still open. A double-click or Enter keeps it and closes,
//! Escape puts back what the field held before it opened.

use core::any::TypeId;
use std::collections::HashMap;

use bevy::asset::Asset;
use bevy::input_focus::InputFocus;
use bevy::picking::events::{Click, Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui_widgets::Activate;
use bevy_fynix::WorldEntityMut;
use bevy_fynix::tag::TagExt as _;
use fynix::prelude::*;
use moxie_asset::{AssetChoices, AssetRef, AssetType, AssetTypes};

use crate::context_menu::at_point;
use bevy::feathers::cursor::EntityCursor;
use bevy::window::SystemCursorIcon;

use crate::elements::{
    Frame, FrameCursor, GhostButton, Icon, Label, LabelCursor,
    MenuSurface, Overlay, ScrollArea, TextField, TintButton,
};
use crate::icons;
use crate::inspector::{ClonableSource, when_changed};
use crate::reactive::{
    BevyUi, component_changed_on, either, resource_changed,
    watch_root,
};
use crate::theme::EditorTheme;
use crate::widgets::tooltip::TooltipExt as _;

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
struct AssetPickerRoot;

/// What the search box holds.
#[derive(Component, Default)]
struct Search(String);

/// Puts back what the field held when the picker opened, and closes
/// it.
#[derive(EntityEvent)]
struct Cancel {
    entity: Entity,
}

/// Asks the app to bring [`AssetChoices`] up to date, before a picker
/// lists them.
#[derive(Event)]
pub struct RefreshAssetChoices;

/// One entry of the grid. `None` clears the field.
#[derive(Clone)]
struct Cell {
    name: String,
    asset: Option<AssetRef>,
    group: String,
    thumbnail: Option<Handle<Image>>,
}

/// "None", unless `T` is [required](AssetType::required), then every
/// [`AssetChoices`] entry for `T`.
fn cells<T: Asset>(world: &mut World) -> Vec<Cell> {
    let kind = TypeId::of::<T>();
    let choices = world
        .get_resource::<AssetChoices>()
        .map(|choices| choices.of::<T>().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let required =
        asset_type(world, kind).is_some_and(|info| info.required);

    let mut cells = Vec::new();
    if !required {
        cells.push(Cell {
            name: "None".to_string(),
            asset: None,
            group: String::new(),
            thumbnail: None,
        });
    }
    for choice in choices {
        let thumbnail = thumbnail(world, kind, &choice.asset);
        cells.push(Cell {
            name: choice.name,
            asset: Some(choice.asset),
            group: choice.group,
            thumbnail,
        });
    }
    cells
}

/// Opens the picker for the `T` that `source` holds, anchored at `at`
/// in logical screen space, closing any picker already open.
pub(crate) fn open_asset_picker<T: Asset + TypePath>(
    world: &mut World,
    at: Vec2,
    source: ClonableSource,
) {
    close_asset_picker(world);
    forget_internal_thumbnails(world);
    world.trigger(RefreshAssetChoices);

    let original = read::<T>(world, &source);
    let title = format!("Select {}", T::short_type_path());

    let root = world
        .spawn((
            AssetPickerRoot,
            Search::default(),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .id();

    let revert = source.clone();
    world.entity_mut(root).observe(
        move |cancel: On<Cancel>, mut commands: Commands| {
            let (source, original, root) =
                (revert.clone(), original.clone(), cancel.entity);
            commands.queue(move |world: &mut World| {
                if let Some(original) = original {
                    source.set(world, &original);
                }
                despawn(world, root);
            });
        },
    );

    watch_root::<EditorTheme>(world, root, move |ui: &mut BevyUi| {
        window::<T>(ui, root, at, &title, &source);
    });
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
    keys: Res<ButtonInput<KeyCode>>,
    roots: Query<Entity, With<AssetPickerRoot>>,
    mut commands: Commands,
) {
    let Ok(root) = roots.single() else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        commands.trigger(Cancel { entity: root });
    } else if keys.just_pressed(KeyCode::Enter) {
        commands.entity(root).despawn();
    }
}

fn read<T: Asset>(
    world: &World,
    source: &ClonableSource,
) -> Option<Handle<T>> {
    Handle::<T>::from_reflect(&*source.get(world)?)
}

/// What `source` holds, if it can be named at all.
fn current<T: Asset>(
    world: &World,
    source: &ClonableSource,
) -> Option<AssetRef> {
    let handle = read::<T>(world, source)?;
    AssetRef::of(&handle, world.get_resource::<AssetServer>()?)
}

fn window<T: Asset>(
    ui: &mut BevyUi,
    root: Entity,
    at: Vec2,
    title: &str,
    source: &ClonableSource,
) {
    let layer = ui.theme.layer.context_menu;
    let margin = ui.theme.space.menu_margin;
    let gap = ui.theme.space.md;

    // Closing on a click elsewhere keeps the pick, like the close
    // button does.
    ui.elem(elem!(Overlay, catches = true, z = layer - 1))
        .observe(
            move |_: On<Pointer<Press>>, mut commands: Commands| {
                commands.entity(root).despawn();
            },
        );

    let (title, source) = (title.to_string(), source.clone());
    ui.elem(elem!(
        Frame,
        position = PositionType::Absolute,
        inset = UiRect::new(px(at.x), auto(), px(at.y), auto()),
    ))
    .with(move |ui| {
        ui.elem(elem!(
            !MenuSurface,
            width = px(WIDTH),
            height = px(HEIGHT),
            row_gap = px(gap),
            padding = UiRect::all(px(gap))
        ))
        .insert(at_point(margin))
        .with(move |ui| {
            header::<T>(ui, root, &title, &source);
            search(ui, root);
            grid::<T>(ui, root, &source);
            footer::<T>(ui, &source);
        });
    });
}

fn header<T: Asset>(
    ui: &mut BevyUi,
    root: Entity,
    title: &str,
    source: &ClonableSource,
) {
    let text = ui.theme.color.text;
    let text_dim = ui.theme.color.text_dim;
    let critical = ui.theme.color.critical;
    let title = title.to_string();
    let create = asset_type(ui.world, TypeId::of::<T>())
        .and_then(|info| info.create);
    let source = source.clone();

    ui.elem(elem!(
        Frame,
        width = percent(100),
        align = AlignItems::Center,
        column_gap = px(4)
    ))
    .with(move |ui| {
        ui.elem(elem!(
            Label,
            text = title,
            bold = true,
            color = text
        ));
        ui.elem(elem!(Frame, flex_grow = 1.0f32));
        if let Some(create) = create {
            ui.elem(elem!(
                !GhostButton,
                icon = elem!(
                    Icon,
                    image = icons::PLUS,
                    color = text_dim,
                    size = px(10)
                ),
                label = elem!(Label, text = "New", color = text)
            ))
            .tooltip("New, starting from the current one")
            .observe(
                move |_: On<Activate>, mut commands: Commands| {
                    let source = source.clone();
                    commands.queue(move |world: &mut World| {
                        let seed = read::<T>(world, &source)
                            .map(|handle| handle.id().untyped());
                        let Some(handle) = create(world, seed) else {
                            return;
                        };
                        source.set(world, &handle.typed::<T>());
                        world.trigger(RefreshAssetChoices);
                    });
                },
            );
        }
        ui.elem(elem!(
            !TintButton {
                tint: Some(critical)
            },
            width = px(14),
            height = px(14),
            padding = UiRect::ZERO,
            radius = px(2),
            icon = elem!(
                Icon,
                image = icons::CLOSE,
                color = text_dim,
                size = px(10)
            )
        ))
        .observe(
            move |_: On<Activate>, mut commands: Commands| {
                commands.entity(root).despawn();
            },
        );
    });
}

fn search(ui: &mut BevyUi, root: Entity) {
    let field = ui.elem(elem!(TextField, width = percent(100)));
    let node = field.id();
    let Some(input) = TextField::text_input(ui.world, node) else {
        return;
    };

    ui.world.insert_resource(InputFocus::from_entity(input));
    ui.world.entity_mut(input).observe(
        move |change: On<TextEditChange>,
              texts: Query<&EditableText>,
              mut searches: Query<&mut Search>| {
            let (Ok(text), Ok(mut search)) = (
                texts.get(change.event_target()),
                searches.get_mut(root),
            ) else {
                return;
            };
            // Also fires on a bare cursor move, which would otherwise
            // rebuild the grid under the pointer.
            let text = text.value().to_string();
            if search.0 != text {
                search.0 = text;
            }
        },
    );
}

fn grid<T: Asset>(
    ui: &mut BevyUi,
    root: Entity,
    source: &ClonableSource,
) {
    let gap = ui.theme.space.sm;
    let text_dim = ui.theme.color.text_dim;
    let source = source.clone();

    ui.elem(elem!(
        ScrollArea,
        width = percent(100),
        flex_grow = 1.0f32,
        scroll_x = false
    ))
    .with(move |ui| {
        let mut grid = ui.elem(elem!(
            Frame,
            width = percent(100),
            direction = FlexDirection::Row,
            row_gap = px(gap),
            column_gap = px(gap)
        ));
        if let Some(mut layout) = grid.entity_mut().get_mut::<Node>()
        {
            layout.flex_wrap = FlexWrap::Wrap;
            layout.align_content = AlignContent::FlexStart;
        }

        let searched_or_listed = either(
            component_changed_on::<Search>(root),
            resource_changed::<AssetChoices>(),
        );
        grid.watch(searched_or_listed, move |ui| {
            let query = ui
                .world
                .get::<Search>(root)
                .map(|search| search.0.trim().to_lowercase())
                .unwrap_or_default();
            let mut group = "";
            let cells = cells::<T>(ui.world);
            for cell in &cells {
                if !cell.name.to_lowercase().contains(&query) {
                    continue;
                }
                if cell.group != group {
                    group = &cell.group;
                    // Full width, so it starts a row of its own.
                    ui.elem(elem!(
                        Frame,
                        width = percent(100),
                        padding = UiRect::top(px(gap))
                    ))
                    .with(move |ui| {
                        ui.elem(elem!(
                            Label,
                            text = cell.group.clone(),
                            size = 11.0f32,
                            color = text_dim
                        ));
                    });
                }
                grid_cell::<T>(ui, root, cell, &source);
            }
        });
    });
}

fn grid_cell<T: Asset>(
    ui: &mut BevyUi,
    root: Entity,
    cell: &Cell,
    source: &ClonableSource,
) {
    let text = ui.theme.color.text;
    let text_dim = ui.theme.color.text_dim;
    let fill = ui.theme.color.fill;
    let hover = ui.theme.color.hover;
    let selection = ui.theme.color.selection;
    let pad = ui.theme.space.sm;

    let shown = source.clone();
    let assign = source.clone();
    let highlighted = cell.asset.clone();
    let assigned = cell.asset.clone();
    let Cell {
        name, thumbnail, ..
    } = cell.clone();
    let empty = cell.asset.is_none();

    let mut frame = ui.elem(elem!(
        Frame,
        width = px(THUMBNAIL + 2.0 * pad),
        direction = FlexDirection::Column,
        align = AlignItems::Center,
        padding = UiRect::all(px(pad)),
        row_gap = px(pad),
        radius = px(4),
        overflow = Overflow::clip(),
        hover_background = Some(hover)
    ));
    frame
        .insert(EntityCursor::System(SystemCursorIcon::Pointer))
        .pointer_tags()
        .bind(
            |frame| frame.background(),
            when_changed(&*source.0),
            move |WorldNodeRef { world, .. }| {
                if current::<T>(world, &shown) == highlighted {
                    selection
                } else {
                    Color::NONE
                }
            },
        )
        .observe(
            move |click: On<Pointer<Click>>,
                  mut commands: Commands| {
                if click.button != PointerButton::Primary {
                    return;
                }
                let double = click.count >= 2;

                let (source, asset) =
                    (assign.clone(), assigned.clone());
                commands.queue(move |world: &mut World| {
                    let handle = match asset {
                        Some(asset) => asset
                            .handle(world.resource::<AssetServer>()),
                        None => Handle::<T>::default(),
                    };
                    source.set(world, &handle);
                    if double {
                        despawn(world, root);
                    }
                });
            },
        )
        .with(move |ui| {
            let mut image = ui.elem(elem!(
                Frame,
                width = px(THUMBNAIL),
                height = px(THUMBNAIL),
                align = AlignItems::Center,
                justify = JustifyContent::Center,
                radius = px(4),
                background = fill
            ));
            match thumbnail {
                Some(thumbnail) => {
                    image.insert(ImageNode::new(thumbnail));
                }
                None if !empty => {
                    image.with(move |ui| {
                        ui.elem(elem!(
                            Icon,
                            image = icons::ASSET,
                            color = text_dim,
                            size = px(20)
                        ));
                    });
                }
                None => {}
            }
            ui.elem(elem!(
                Label,
                text = name,
                size = 11.0f32,
                color = text
            ));
        });
}

/// The pick, by name and where it comes from.
fn footer<T: Asset>(ui: &mut BevyUi, source: &ClonableSource) {
    let text_dim = ui.theme.color.text_dim;
    let shown = source.clone();
    let describe = move |world: &World| {
        let Some(asset) = current::<T>(world, &shown) else {
            return "None".to_string();
        };
        let name = world.get_resource::<AssetChoices>().and_then(
            |choices| {
                choices
                    .of::<T>()
                    .find(|choice| choice.asset == asset)
                    .map(|choice| choice.name.clone())
            },
        );
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
    };
    let text = describe(ui.world);

    ui.elem(elem!(
        Label,
        text = text,
        size = 11.0f32,
        wrap = false,
        color = text_dim
    ))
    .bind(
        |label| label.text(),
        when_changed(&*source.0),
        move |WorldNodeRef { world, .. }| describe(world),
    );
}
