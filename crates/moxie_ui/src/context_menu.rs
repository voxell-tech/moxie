//! A right-click menu: a small popup of actions at the cursor,
//! dismissed by clicking anywhere else.
//!
//! Spawned as plain bundles rather than through [`crate::reactive`]'s
//! composer: it is a one-shot popup with nothing to react to once
//! built, the same reasoning [`crate::drag::ghost`] follows.

use bevy::picking::events::{Out, Over, Pointer, Press};
use bevy::picking::pointer::{PointerButton, PointerLocation};
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy_fynix::{BevyFynix, WorldEntityMut};

use crate::theme::EditorTheme;

/// The open menu's own nodes, so a second right-click - or the
/// catch-all overlay behind it - can close it before anything else
/// happens.
#[derive(Component)]
struct ContextMenuRoot;

/// Adds one row to a [`context_menu`], to run `on_click` and close
/// the menu when picked.
pub struct ContextMenuBuilder<'w> {
    world: &'w mut World,
    list: Entity,
    theme: EditorTheme,
}

impl ContextMenuBuilder<'_> {
    pub fn item(
        &mut self,
        label: impl Into<String>,
        on_click: impl Fn(&mut World) + Send + Sync + 'static,
    ) {
        let text = self.theme.color.text;
        let hover = self.theme.color.hover;

        let mut row = self.world.spawn((
            ChildOf(self.list),
            Node {
                width: percent(100),
                padding: UiRect::axes(px(10), px(5)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            children![(
                Text::new(label.into()),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(text),
                TextLayout::linebreak(LineBreak::NoWrap),
                Pickable::IGNORE,
            )],
        ));
        let node = row.id();

        row
            .observe(
                move |_: On<Pointer<Over>>,
                      mut backgrounds: Query<
                    &mut BackgroundColor,
                >| {
                    if let Ok(mut background) =
                        backgrounds.get_mut(node)
                    {
                        background.0 = hover;
                    }
                },
            )
            .observe(
                move |_: On<Pointer<Out>>,
                      mut backgrounds: Query<
                    &mut BackgroundColor,
                >| {
                    if let Ok(mut background) =
                        backgrounds.get_mut(node)
                    {
                        background.0 = Color::NONE;
                    }
                },
            )
            .observe(
                move |_: On<Pointer<Press>>, world: &mut World| {
                    despawn_context_menu(world);
                    on_click(world);
                },
            );
    }
}

/// Makes `elem` open `build`'s rows at the cursor on right-click -
/// reusable for delete, duplicate, or whatever else a row offers.
pub fn context_menu(
    elem: &mut impl WorldEntityMut,
    build: impl Fn(&mut ContextMenuBuilder) + Send + Sync + 'static,
) {
    elem.observe(
        move |press: On<Pointer<Press>>, world: &mut World| {
            if press.button != PointerButton::Secondary {
                return;
            }

            let scale = world.resource::<UiScale>().0;
            let Some(at) = world
                .query::<&PointerLocation>()
                .iter(world)
                .find_map(|pointer| pointer.location())
                .map(|location| location.position / scale)
            else {
                return;
            };
            let theme =
                world.resource::<BevyFynix<EditorTheme>>().theme();
            let theme = theme.clone();

            spawn_context_menu(world, at, theme, &build);
        },
    );
}

/// Closes whatever [`context_menu`] is currently open, if any.
fn despawn_context_menu(world: &mut World) {
    let open: Vec<Entity> = world
        .query_filtered::<Entity, With<ContextMenuRoot>>()
        .iter(world)
        .collect();
    for entity in open {
        world.despawn(entity);
    }
}

fn spawn_context_menu(
    world: &mut World,
    at: Vec2,
    theme: EditorTheme,
    build: &(impl Fn(&mut ContextMenuBuilder) + Send + Sync + 'static),
) {
    despawn_context_menu(world);

    // Catches a click anywhere else, closing the menu without acting
    // on whatever it landed on.
    world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                ..default()
            },
            GlobalZIndex(theme.layer.context_menu - 1),
            ContextMenuRoot,
        ))
        .observe(|_: On<Pointer<Press>>, world: &mut World| {
            despawn_context_menu(world);
        });

    let list = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(at.x),
                top: px(at.y),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(4)),
                min_width: px(120),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(theme.color.panel),
            GlobalZIndex(theme.layer.context_menu),
            ContextMenuRoot,
        ))
        .id();

    let mut builder = ContextMenuBuilder { world, list, theme };
    build(&mut builder);
}
