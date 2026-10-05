//! The user's own keys, saved apart from any project.
//!
//! Only what differs from the keys commands were registered with is
//! kept, so a new default reaches whoever did not touch that command.

use bevy::prelude::*;
use bevy::settings::{
    ReflectSettingsGroup, SaveSettingsDeferred, SettingsGroup,
    SettingsPlugin,
};
use bevy_fynix::shortcut::{Chord, CommandId, CommandList, Keymap};

pub(crate) fn plugin(app: &mut App) {
    app.register_type::<KeymapSettings>()
        .init_resource::<KeymapSettings>()
        .add_systems(Startup, apply);
}

/// The plugin that loads and saves the editor's settings. It is
/// added after [`MoxiePlugin`](crate::MoxiePlugin), which registers
/// what it keeps.
pub fn settings_plugin() -> SettingsPlugin {
    SettingsPlugin::new("org.voxell.moxie")
}

/// The user's changes to the keymap.
#[derive(Resource, SettingsGroup, Reflect, Debug, Default)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(file = "keymap")]
pub struct KeymapSettings {
    overrides: Vec<KeyOverride>,
}

/// The chords one command is bound to.
#[derive(Reflect, Debug, Default, Clone, PartialEq)]
#[reflect(Default, Clone)]
pub struct KeyOverride {
    command: String,
    chords: Vec<Chord>,
}

/// Puts the saved keys over the registered ones. A saved command
/// nothing registered is left alone: its plugin may not be loaded.
fn apply(
    settings: Res<KeymapSettings>,
    list: Res<CommandList>,
    mut keymap: ResMut<Keymap>,
) {
    for saved in &settings.overrides {
        let command = list
            .commands()
            .iter()
            .find(|command| command.id.0 == saved.command);
        if let Some(command) = command {
            keymap.rebind(command.id, saved.chords.clone());
        }
    }
}

/// Binds `command` to `chords` alone, or with `None` puts it back on
/// its registered keys, and saves the change.
pub(crate) fn rebind(
    world: &mut World,
    command: CommandId,
    chords: Option<Vec<Chord>>,
) {
    let mut settings = world.resource_mut::<KeymapSettings>();
    settings
        .overrides
        .retain(|saved| saved.command != command.0);
    if let Some(chords) = &chords {
        settings.overrides.push(KeyOverride {
            command: command.0.to_string(),
            chords: chords.clone(),
        });
    }
    let mut keymap = world.resource_mut::<Keymap>();
    match chords {
        Some(chords) => keymap.rebind(command, chords),
        None => keymap.reset(command),
    }
    world.commands().queue(SaveSettingsDeferred::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::harness::Editor;

    #[test]
    fn the_shipped_keys_do_not_conflict() {
        let mut editor = Editor::new();
        let world = editor.world();
        let conflicts = world
            .resource::<Keymap>()
            .conflicts(world.resource::<CommandList>());
        assert_eq!(conflicts, []);
    }

    #[test]
    fn a_saved_key_for_an_unknown_command_is_kept() {
        const SAVE: CommandId = CommandId("file.save");
        let mut editor = Editor::new();
        let world = editor.world();
        world.resource_mut::<KeymapSettings>().overrides.push(
            KeyOverride {
                command: "plugin.absent".into(),
                chords: vec![Chord::key(KeyCode::KeyK)],
            },
        );

        let k = Chord::key(KeyCode::KeyK);
        rebind(world, SAVE, Some(vec![k]));
        let chords = world
            .resource::<Keymap>()
            .chords(SAVE)
            .collect::<Vec<_>>();
        assert_eq!(chords, [k]);

        rebind(world, SAVE, None);
        let settings = world.resource::<KeymapSettings>();
        assert_eq!(settings.overrides.len(), 1, "only the unknown");
        assert_eq!(settings.overrides[0].command, "plugin.absent");
    }
}
