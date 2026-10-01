//! The menu bar above the dock, holding what acts on the project as a
//! whole rather than on anything a panel is showing.

use bevy::prelude::*;
use bevy_fynix::views::{FrameProps as _, dropdown, row};
use bevy_fynix::{AnyView, Bevy, Cx, View, ViewExt as _};
use moxie_ui::icons;
use moxie_ui::theme::EditorTheme;

use crate::project;

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
    .align(AlignItems::Center)
}

/// One menu: its name in the bar, and what picking an entry runs.
///
/// The name is the dropdown's placeholder, so the control keeps it
/// whichever entry was picked.
fn menu(
    name: &'static str,
    entries: Vec<(&'static str, fn(&mut World))>,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::new(move |cx: &mut Cx<'_, Bevy, EditorTheme>| {
        let chevron =
            cx.world.resource::<AssetServer>().load(icons::CHEVRON);
        cx.build(
            dropdown(
                entries
                    .iter()
                    .map(|(entry, _)| *entry)
                    .collect::<Vec<_>>(),
                usize::MAX,
                chevron,
                move |world, at| {
                    if let Some((_, run)) = entries.get(at) {
                        run(world);
                    }
                },
            )
            .placeholder(name),
        )
    })
}
