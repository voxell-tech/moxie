//! A right-click menu: a small popup of [`DropdownItem`] rows at the
//! cursor, dismissed by clicking anywhere else - the same row element
//! every other menu in the app uses (`Dropdown`'s own list, the enum
//! variant picker, `AddComponent`, the top bar's File menu), so a
//! right-click menu reads like the rest rather than like a one-off.

use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::{PointerButton, PointerLocation};
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy::ui_widgets::Activate;
use bevy_fynix::WorldEntityMut;
use bevy_fynix::tag::TagExt as _;
use fynix::prelude::*;

use crate::elements::{DropdownItem, Frame, Label, Overlay};
use crate::reactive::{BevyUi, watch_root};
use crate::theme::EditorTheme;

/// The open menu's own root, so a second right-click - or the
/// catch-all overlay behind it - can close it before anything else
/// happens.
#[derive(Component)]
struct ContextMenuRoot;

/// Adds one row to a [`context_menu`], to run `on_click` and close
/// the menu when picked.
pub struct ContextMenuBuilder<'u, 'a> {
    ui: &'u mut BevyUi<'a>,
}

impl ContextMenuBuilder<'_, '_> {
    pub fn item(
        &mut self,
        label: impl Into<String>,
        on_click: impl Fn(&mut World) + Send + Sync + Clone + 'static,
    ) {
        let text = self.ui.theme.color.text;

        self.ui
            .elem(elem!(
                DropdownItem,
                label = elem!(
                    Label,
                    text = label.into(),
                    wrap = false,
                    color = text
                )
            ))
            .pointer_tags()
            .observe(
                move |_: On<Activate>, mut commands: Commands| {
                    let on_click = on_click.clone();
                    commands.queue(despawn_context_menu);
                    commands.queue(move |world: &mut World| {
                        on_click(world);
                    });
                },
            );
    }
}

/// Makes `elem` open `build`'s rows at the cursor on right-click -
/// reusable for delete, duplicate, or whatever else a row offers.
pub fn context_menu(
    elem: &mut impl WorldEntityMut,
    build: impl Fn(&mut ContextMenuBuilder)
    + Send
    + Sync
    + Clone
    + 'static,
) {
    elem.observe(
        move |press: On<Pointer<Press>>,
              scale: Res<UiScale>,
              pointers: Query<&PointerLocation>,
              mut commands: Commands| {
            if press.button != PointerButton::Secondary {
                return;
            }
            let Some(at) = pointers
                .iter()
                .find_map(|pointer| pointer.location())
                .map(|location| location.position / scale.0)
            else {
                return;
            };

            let build = build.clone();
            commands.queue(move |world: &mut World| {
                spawn_context_menu(world, at, build);
            });
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
    build: impl Fn(&mut ContextMenuBuilder)
    + Send
    + Sync
    + Clone
    + 'static,
) {
    despawn_context_menu(world);

    // A `Node` of its own, same as the app's own UI root: without
    // one this is a plain entity, and every UI child parented under
    // it inherits no layout at all. No `UiTargetCamera` needed -
    // like a drag ghost or drop overlay, it falls back to whichever
    // camera is marked `IsDefaultUiCamera`.
    let root = world
        .spawn((
            ContextMenuRoot,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .id();
    watch_root::<EditorTheme>(world, root, move |ui: &mut BevyUi| {
        let layer = ui.theme.layer.context_menu;

        // Catches a click anywhere else, closing the menu without
        // acting on whatever it landed on.
        ui.elem(elem!(Overlay, catches = true, z = layer - 1))
            .observe(
                |_: On<Pointer<Press>>, mut commands: Commands| {
                    commands.queue(despawn_context_menu);
                },
            );

        let background = ui.theme.color.panel;
        let build = build.clone();
        ui.elem(elem!(
            Frame,
            position = PositionType::Absolute,
            inset = UiRect::new(px(at.x), auto(), px(at.y), auto()),
            min_width = px(120),
            direction = FlexDirection::Column,
            padding = UiRect::all(px(4)),
            background = background,
            radius = px(4),
            z = Some(layer)
        ))
        .with(move |ui| {
            let mut builder = ContextMenuBuilder { ui };
            build(&mut builder);
        });
    });
}
