//! The shortcuts panel: every command by the place it belongs to,
//! with the keys it is bound to, a way to bind it to another and a
//! way back to the key it came with.

use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy_fynix::dock::DockTree;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandList, CommandSpec, GLOBAL, Keymap,
    Layer, Mods, ScopeId, ScopeSpec, ShortcutAppExt as _,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, button, column, frame, ghost,
    label, row, scroll,
};
use bevy_fynix::{
    AnyView, Bevy, ScopedExt as _, View, ViewExt as _, keyed,
};
use moxie_ui::gaps::changing;
use moxie_ui::theme::EditorTheme;

use crate::keymap;

/// The name the dock knows the panel by.
pub(super) const WINDOW: &str = "shortcuts";

/// The command that brings the panel up.
pub(super) const OPEN: CommandId = CommandId("window.shortcuts");

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Capturing>()
        .add_scope(ScopeSpec {
            id: ScopeId("shortcuts.capture"),
            label: "Binding a key",
            layer: Layer::Gesture,
            // No command of its own: it keeps the key being bound
            // from running whatever has it now.
            active: Some(|world| {
                world.get_resource::<Capturing>().is_some_and(
                    |capturing| {
                        capturing.command.is_some() || capturing.held
                    },
                )
            }),
        })
        .add_command(
            CommandSpec {
                id: OPEN,
                label: "Keyboard shortcuts",
                scope: GLOBAL,
                run: |world, _| open(world),
                enabled: |_| true,
                repeat: false,
            },
            &[],
        )
        .add_observer(capture);
}

/// Shows the panel, beside the project's when it is not open yet.
fn open(world: &mut World) {
    let mut tree = world.resource_mut::<DockTree>();
    let shown = tree
        .tabs()
        .find(|(_, tab)| tab.window_id == WINDOW)
        .map(|(leaf, tab)| (leaf, tab.id));
    if let Some((leaf, tab)) = shown {
        tree.set_active(leaf, tab);
        return;
    }
    let leaf = tree
        .find_leaf_with_window("project")
        .or_else(|| tree.leaves().next().map(|(leaf, _)| leaf));
    if let Some(leaf) = leaf {
        tree.add_tab(leaf, WINDOW);
    }
}

/// The command waiting for the key it will be bound to, if any.
#[derive(Resource, Default)]
struct Capturing {
    command: Option<CommandId>,
    /// The key that ended the wait is still down. Until it is up
    /// the scope stays in force, or the press that binds a command
    /// would run it too.
    held: bool,
}

/// Binds the command being rebound to the next key pressed, with
/// what is held as it goes down. Escape leaves it as it was.
fn capture(
    event: On<FocusedInput<KeyboardInput>>,
    windows: Query<(), With<Window>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut capturing: ResMut<Capturing>,
    mut commands: Commands,
) {
    let key = &event.input;
    if !windows.contains(event.event_target()) {
        return;
    }
    if !key.state.is_pressed() {
        capturing.held = false;
        return;
    }
    let Some(command) = capturing.command else {
        return;
    };
    if key.repeat {
        return;
    }
    let chord = Chord {
        key: key.key_code,
        mods: Mods::held(&keys),
    };
    if chord == Chord::key(KeyCode::Escape) {
        *capturing = Capturing {
            command: None,
            held: true,
        };
        return;
    }
    // A modifier on its way down, or a key with no name of its own.
    if matches!(
        chord.key,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
            | KeyCode::Unidentified(_)
    ) {
        return;
    }
    *capturing = Capturing {
        command: None,
        held: true,
    };
    commands.queue(move |world: &mut World| {
        keymap::rebind(world, command, Some(vec![chord]));
    });
}

/// The commands of one scope.
#[derive(Clone, PartialEq)]
struct Section {
    label: &'static str,
    rows: Vec<Row>,
}

#[derive(Clone, PartialEq)]
struct Row {
    command: CommandId,
    label: &'static str,
    keys: String,
    /// Whether the user bound it themselves.
    rebound: bool,
    /// Whether it is waiting for its key.
    capturing: bool,
    /// Whether another command of its scope has one of its keys.
    clashes: bool,
}

fn sections(world: &World) -> Vec<Section> {
    let list = world.resource::<CommandList>();
    let keymap = world.resource::<Keymap>();
    let capturing = world.resource::<Capturing>().command;
    let conflicts = keymap.conflicts(list);
    list.scopes()
        .iter()
        .map(|scope| Section {
            label: scope.label,
            rows: list
                .commands()
                .iter()
                .filter(|command| command.scope == scope.id)
                .map(|command| Row {
                    command: command.id,
                    label: command.label,
                    keys: keymap
                        .chords(command.id)
                        .map(|chord| chord.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    rebound: keymap.rebound(command.id).is_some(),
                    capturing: capturing == Some(command.id),
                    clashes: conflicts.iter().any(|conflict| {
                        conflict.commands.contains(&command.id)
                    }),
                })
                .collect(),
        })
        .filter(|section| !section.rows.is_empty())
        .collect()
}

/// The shortcuts panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        cx.build(
            scroll((keyed::<EditorTheme, Vec<Section>>(
                changing(sections),
                |sections| list(sections.clone()),
            ),))
            .width(percent(100.0))
            .height(percent(100.0))
            .padding(UiRect::all(px(pad))),
        )
    })
}

fn list(sections: Vec<Section>) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let space = cx.theme().space;
        let mut items = Vec::new();
        for section in &sections {
            items.push(label(section.label).bold(true).boxed());
            for entry in &section.rows {
                items.push(line(entry.clone(), space.row).boxed());
            }
        }
        cx.build(column(items).width(percent(100.0)).gap(space.sm))
    })
}

/// One command: its name, its keys, and the buttons that change
/// them.
fn line(entry: Row, height: f32) -> impl View<Bevy, EditorTheme> {
    let command = entry.command;
    let (keys, tone) = if entry.capturing {
        ("Press a key".to_string(), Tone::Accent)
    } else if entry.keys.is_empty() {
        ("None".to_string(), Tone::Faint)
    } else if entry.clashes {
        // Only the first command on a key answers it.
        (format!("{} (shared)", entry.keys), Tone::Critical)
    } else {
        (entry.keys, Tone::Dim)
    };
    let rebind = button(label("Rebind"))
        .height(px(height))
        .rules(ghost)
        .on_activate(move |world| {
            // Or the key lands on the button just pressed.
            world.resource_mut::<InputFocus>().clear();
            world.resource_mut::<Capturing>().command = Some(command);
        });
    let mut items = vec![
        label(entry.label).boxed(),
        frame().grow(1.0).boxed(),
        label(keys).tone(tone).boxed(),
        rebind.boxed(),
    ];
    if entry.rebound {
        items.push(
            button(label("Reset"))
                .height(px(height))
                .rules(ghost)
                .on_activate(move |world| {
                    keymap::rebind(world, command, None);
                })
                .boxed(),
        );
    }
    row(items)
        .width(percent(100.0))
        .height(px(height))
        .align(AlignItems::Center)
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::Key;
    use bevy_fynix::shortcut::{Invoke, run_command};

    use super::*;
    use crate::playback::{TOGGLE_PLAYBACK, TogglePlayback};
    use crate::tests::harness::{Editor, SETTLE};

    #[test]
    fn a_command_takes_the_next_key_pressed() {
        let mut editor = Editor::new();
        // A blank project has nothing to play, so the command is
        // counted as it is asked for.
        editor.world().init_resource::<Toggled>();
        editor.world().add_observer(
            |_: On<TogglePlayback>, mut toggled: ResMut<Toggled>| {
                toggled.0 += 1;
            },
        );
        run_command(editor.world(), OPEN, Invoke::default());
        editor.step(SETTLE);
        assert!(!editor.texts("Play or pause").is_empty());

        let toggle = TOGGLE_PLAYBACK.id;
        editor.world().resource_mut::<Capturing>().command =
            Some(toggle);
        let k = || Key::Character("k".into());
        editor.tap(KeyCode::KeyK, k());
        let world = editor.world();
        assert_eq!(
            world
                .resource::<Keymap>()
                .chords(toggle)
                .collect::<Vec<_>>(),
            [Chord::key(KeyCode::KeyK)]
        );
        assert_eq!(world.resource::<Toggled>().0, 0);

        editor.tap(KeyCode::Space, Key::Space);
        assert_eq!(
            editor.world().resource::<Toggled>().0,
            0,
            "its old key is free"
        );
        editor.tap(KeyCode::KeyK, k());
        assert_eq!(editor.world().resource::<Toggled>().0, 1);
    }

    #[derive(Resource, Default)]
    struct Toggled(u32);
}
