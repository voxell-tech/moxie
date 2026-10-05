//! The menu bar above the dock, holding what acts on the project as a
//! whole rather than on anything a panel is showing.

use bevy::prelude::*;
use bevy_fynix::views::{FrameProps as _, menu_button, row};
use bevy_fynix::{Bevy, View};
use moxie_ui::theme::EditorTheme;

use crate::project;

const BAR_HEIGHT: f32 = 26.0;

/// The bar: one menu per heading.
pub(super) fn top_bar() -> impl View<Bevy, EditorTheme> {
    row((menu(
        "File",
        vec![
            ("New", project::new_scene),
            ("Open", project::load_scene),
            ("Save", project::save_scene),
        ],
    ),))
    .width(percent(100.0))
    .height(px(BAR_HEIGHT))
    // Or a dock with much in it squeezes the bar.
    .shrink(0.0)
    .align(AlignItems::Center)
}

/// One menu: its name in the bar, and what picking an entry runs.
fn menu(
    name: &'static str,
    entries: Vec<(&'static str, fn(&mut World))>,
) -> impl View<Bevy, EditorTheme> {
    menu_button(
        name,
        entries.iter().map(|(entry, _)| *entry).collect::<Vec<_>>(),
        move |world, at| {
            if let Some((_, run)) = entries.get(at) {
                run(world);
            }
        },
    )
}
