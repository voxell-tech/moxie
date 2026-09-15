//! A row's own delete control, invisible until the row it belongs to
//! is hovered.

use bevy::picking::events::{Out, Over, Pointer};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy_fynix::WorldEntityMut as _;
use bevy_fynix::tag::{Hovered, TagExt as _};
use fynix::prelude::*;

use crate::elements::{ButtonCursor as _, Icon, TintButton};
use crate::reactive::BevyUi;

/// A small delete button that stays invisible until `row` is hovered,
/// then fades in - `row` is the entity whose hover should reveal it,
/// typically the header or list row this button sits inside rather
/// than the row itself.
pub fn hover_delete(
    ui: &mut BevyUi,
    row: Entity,
    icon: &str,
    on_click: impl Fn(&mut World) + Send + Sync + Clone + 'static,
) {
    let critical = ui.theme.color.critical;
    let mut button = ui.elem(elem!(
        !TintButton {
            tint: Some(critical)
        },
        width = px(14),
        height = px(14),
        padding = UiRect::ZERO,
        radius = px(2),
        icon = elem!(
            Icon,
            image = icon.to_string(),
            size = px(10),
            color = Color::NONE
        )
    ));
    // The icon, not the button: `TintButton`'s `Hover::IconLabel`
    // reads the icon's own tag, not the button's.
    if let Some(icon) = button.child(|button| button.icon()) {
        button
            .tag_node_from::<Pointer<Over>, _>(icon, row, Hovered)
            .untag_node_from::<Pointer<Out>, Hovered>(icon, row);
    }
    button.observe(move |_: On<Activate>, mut commands: Commands| {
        let on_click = on_click.clone();
        commands.queue(move |world: &mut World| on_click(world));
    });
}
