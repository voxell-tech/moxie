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
use moxie_asset::AssetChoices;

use crate::context_menu::at_point;
use crate::elements::{
    Frame, FrameCursor, GhostButton, Icon, Label, LabelCursor,
    MenuSurface, Overlay, ScrollArea, TextField,
};
use crate::icons;
use crate::inspector::{ClonableSource, when_changed};
use crate::reactive::{
    BevyUi, FynixHost, component_changed_on, resource_changed,
    watch_root,
};
use crate::theme::EditorTheme;

const WIDTH: f32 = 372.0;
const HEIGHT: f32 = 440.0;
const THUMBNAIL: f32 = 64.0;
/// Seconds between two clicks on one cell for them to count as a
/// double-click.
const DOUBLE_CLICK: f64 = 0.5;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<AssetThumbnails>()
        .add_systems(Update, picker_keys);
}

/// Renders a preview of the asset at a path into an image, or `None`
/// when it can't.
pub type RenderThumbnail =
    fn(&mut World, &str) -> Option<Handle<Image>>;

/// Thumbnail renderers by asset type, and every thumbnail rendered so
/// far.
#[derive(Resource, Default)]
pub struct AssetThumbnails {
    renderers: HashMap<TypeId, RenderThumbnail>,
    rendered: HashMap<(TypeId, String), Handle<Image>>,
}

/// Registering how an asset type's thumbnail is rendered.
pub trait AssetThumbnailAppExt {
    fn register_asset_thumbnail<T: Asset>(
        &mut self,
        render: RenderThumbnail,
    ) -> &mut Self;
}

impl AssetThumbnailAppExt for App {
    fn register_asset_thumbnail<T: Asset>(
        &mut self,
        render: RenderThumbnail,
    ) -> &mut Self {
        self.world_mut()
            .get_resource_or_insert_with(AssetThumbnails::default)
            .renderers
            .insert(TypeId::of::<T>(), render);
        self
    }
}

/// The thumbnail for the `kind` asset at `path`, rendered on first
/// ask and reused after.
fn thumbnail(
    world: &mut World,
    kind: TypeId,
    path: &str,
) -> Option<Handle<Image>> {
    let key = (kind, path.to_string());
    let thumbnails = world.get_resource::<AssetThumbnails>()?;
    if let Some(image) = thumbnails.rendered.get(&key) {
        return Some(image.clone());
    }
    let render = *thumbnails.renderers.get(&kind)?;

    let image = render(world, path)?;
    world
        .resource_mut::<AssetThumbnails>()
        .rendered
        .insert(key, image.clone());
    Some(image)
}

/// The open picker's own root. There is at most one.
#[derive(Component)]
struct AssetPickerRoot;

/// What the search box holds.
#[derive(Component, Default)]
struct Search(String);

/// The last cell clicked and when, to tell a double-click.
#[derive(Component, Default)]
struct LastClick {
    cell: Option<usize>,
    at: f64,
}

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

/// One entry of the grid. A `None` path clears the field.
#[derive(Clone)]
struct Cell {
    name: String,
    path: Option<String>,
    group: String,
    thumbnail: Option<Handle<Image>>,
}

/// "None", then every [`AssetChoices`] entry for `T`.
fn cells<T: Asset>(world: &mut World) -> Vec<Cell> {
    let kind = TypeId::of::<T>();
    let choices = world
        .get_resource::<AssetChoices>()
        .map(|choices| choices.of::<T>().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    let mut cells = vec![Cell {
        name: "None".to_string(),
        path: None,
        group: String::new(),
        thumbnail: None,
    }];
    for choice in choices {
        let thumbnail = thumbnail(world, kind, &choice.path);
        cells.push(Cell {
            name: choice.name,
            path: Some(choice.path),
            group: choice.group,
            thumbnail,
        });
    }
    cells
}

/// Loads the asset at `path`, which may lie outside the default asset
/// source.
pub fn load_choice<T: Asset>(world: &World, path: &str) -> Handle<T> {
    world
        .resource::<AssetServer>()
        .load_builder()
        .override_unapproved()
        .load::<T>(path.to_string())
}

/// Opens the picker for the `T` that `source` holds, anchored at `at`
/// in logical screen space, closing any picker already open.
pub(crate) fn open_asset_picker<T: Asset + TypePath>(
    world: &mut World,
    at: Vec2,
    source: ClonableSource,
) {
    close_asset_picker(world);
    world.trigger(RefreshAssetChoices);

    let original = read::<T>(world, &source);
    let title = format!("Select {}", T::short_type_path());

    let root = world
        .spawn((
            AssetPickerRoot,
            Search::default(),
            LastClick::default(),
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

/// The asset path `source` holds, if it holds a named asset.
fn current_path<T: Asset>(
    world: &World,
    source: &ClonableSource,
) -> Option<String> {
    let handle = read::<T>(world, source)?;
    let path =
        world.get_resource::<AssetServer>()?.get_path(&handle)?;
    Some(path.to_string())
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
            header(ui, root, &title);
            search(ui, root);
            grid::<T>(ui, root, &source);
            footer::<T>(ui, &source);
        });
    });
}

fn header(ui: &mut BevyUi, root: Entity, title: &str) {
    let text = ui.theme.color.text;
    let text_dim = ui.theme.color.text_dim;
    let title = title.to_string();

    ui.elem(elem!(
        Frame,
        width = percent(100),
        align = AlignItems::Center,
        justify = JustifyContent::SpaceBetween
    ))
    .with(move |ui| {
        ui.elem(elem!(
            Label,
            text = title,
            bold = true,
            color = text
        ));
        ui.elem(elem!(
            !GhostButton,
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

        grid.watch(search_or_choices_changed(root), move |ui| {
            let query = ui
                .world
                .get::<Search>(root)
                .map(|search| search.0.trim().to_lowercase())
                .unwrap_or_default();
            let mut group = "";
            let cells = cells::<T>(ui.world);
            for (index, cell) in cells.iter().enumerate() {
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
                grid_cell::<T>(ui, root, index, cell, &source);
            }
        });
    });
}

/// Fires on either, since the grid lists [`AssetChoices`] filtered by
/// the [`Search`].
fn search_or_choices_changed(
    root: Entity,
) -> impl for<'w> FnMut(WorldNodeRef<'w, FynixHost>) -> bool
+ Send
+ Sync
+ 'static {
    let mut searched = component_changed_on::<Search>(root);
    let mut listed = resource_changed::<AssetChoices>();
    move |WorldNodeRef { world, node }| {
        let searched = searched(WorldNodeRef::new(world, node));
        let listed = listed(WorldNodeRef::new(world, node));
        searched || listed
    }
}

fn grid_cell<T: Asset>(
    ui: &mut BevyUi,
    root: Entity,
    index: usize,
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
    let highlighted = cell.path.clone();
    let assigned = cell.path.clone();
    let Cell {
        name, thumbnail, ..
    } = cell.clone();
    let empty = cell.path.is_none();

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
        .pointer_tags()
        .bind(
            |frame| frame.background(),
            when_changed(&*source.0),
            move |WorldNodeRef { world, .. }| {
                if current_path::<T>(world, &shown) == highlighted {
                    selection
                } else {
                    Color::NONE
                }
            },
        )
        .observe(
            move |click: On<Pointer<Click>>,
                  time: Res<Time>,
                  mut last: Query<&mut LastClick>,
                  mut commands: Commands| {
                if click.button != PointerButton::Primary {
                    return;
                }
                let now = time.elapsed_secs_f64();
                let double =
                    last.get_mut(root).is_ok_and(|mut last| {
                        let double = last.cell == Some(index)
                            && now - last.at < DOUBLE_CLICK;
                        *last = LastClick {
                            cell: Some(index),
                            at: now,
                        };
                        double
                    });

                let (source, path) =
                    (assign.clone(), assigned.clone());
                commands.queue(move |world: &mut World| {
                    let handle = match path {
                        Some(path) => load_choice::<T>(world, &path),
                        None => Handle::default(),
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

/// The pick, by name and path.
fn footer<T: Asset>(ui: &mut BevyUi, source: &ClonableSource) {
    let text_dim = ui.theme.color.text_dim;
    let shown = source.clone();
    let describe = move |world: &World| {
        let Some(path) = current_path::<T>(world, &shown) else {
            return "None".to_string();
        };
        let name = world.get_resource::<AssetChoices>().and_then(
            |choices| {
                choices
                    .of::<T>()
                    .find(|choice| choice.path == path)
                    .map(|choice| choice.name.clone())
            },
        );
        match name {
            Some(name) => format!("{name}  {path}"),
            None => path,
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
