//! The menu bar above the dock, holding what acts on the project as a
//! whole rather than on anything a panel is showing.

use bevy::prelude::*;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Invoke, Keymap, Mods,
    ShortcutAppExt as _, run_command,
};
use bevy_fynix::views::{
    FrameProps as _, MenuEntry, menu_button, row,
};
use bevy_fynix::{AnyView, Bevy, Keyed, View, keyed, resource};
use moxie_ui::theme::EditorTheme;

use crate::project;

const BAR_HEIGHT: f32 = 26.0;

/// The commands of the File menu, in its order.
const FILE: [CommandId; 4] = [
    CommandId("file.new"),
    CommandId("file.open"),
    CommandId("file.save"),
    super::shortcuts::OPEN,
];

/// Registers the commands the bar's menus run.
pub(super) fn plugin(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id,
        label,
        scope: GLOBAL,
        run,
        enabled: |_| true,
        repeat: false,
    };
    let primary = |key| Chord {
        key,
        mods: Mods::PRIMARY,
    };
    app.add_command(
        command(FILE[0], "New", |world, _| project::new_scene(world)),
        &[primary(KeyCode::KeyN)],
    )
    .add_command(
        command(FILE[1], "Open", |world, _| {
            project::load_scene(world);
        }),
        &[primary(KeyCode::KeyO)],
    )
    .add_command(
        command(FILE[2], "Save", |world, _| {
            project::save_scene(world);
        }),
        &[primary(KeyCode::KeyS)],
    );
}

/// The bar: one menu per heading.
pub(super) fn top_bar() -> impl View<Bevy, EditorTheme> {
    row((menu("File", &FILE),))
        .width(percent(100.0))
        .height(px(BAR_HEIGHT))
        // Or a dock with much in it squeezes the bar.
        .shrink(0.0)
        .align(AlignItems::Center)
}

/// One menu: its name in the bar, and a row for each of `commands`
/// with the key it is bound to, built again when one is rebound.
fn menu(
    name: &'static str,
    commands: &'static [CommandId],
) -> Keyed<EditorTheme, Vec<Option<Chord>>> {
    keyed(
        resource::<Keymap, _>(move |keymap| {
            commands
                .iter()
                .map(|&command| keymap.chords(command).next())
                .collect::<Vec<_>>()
        }),
        move |_| entries(name, commands),
    )
}

fn entries(
    name: &'static str,
    commands: &'static [CommandId],
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let entries = commands
            .iter()
            .filter_map(|&command| {
                MenuEntry::command(cx.world, command)
            })
            .collect::<Vec<_>>();
        cx.build(menu_button(name, entries, move |world, at| {
            if let Some(&command) = commands.get(at) {
                run_command(world, command, Invoke::default());
            }
        }))
    })
}
