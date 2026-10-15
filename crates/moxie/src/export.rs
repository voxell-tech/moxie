//! Rendering the timeline to a video or a PNG sequence.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Mods, ShortcutAppExt as _,
};
use moxie_export::{
    Codec, Export, ExportFinished, Output, find_ffmpeg, frame_count,
};

use crate::{EditorState, PreviewImage, ProjectSettings};

/// Exports the whole timeline to a file picked for it.
pub(crate) const EXPORT: CommandId = CommandId("file.export");

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(moxie_export::plugin)
        .add_observer(on_finished)
        .add_command(
            CommandSpec {
                id: EXPORT,
                label: "Export",
                scope: GLOBAL,
                run: |world, _| export_timeline(world),
                enabled: |world| {
                    world.resource::<EditorState>().timeline.is_some()
                },
                repeat: false,
            },
            &[Chord {
                key: KeyCode::KeyE,
                mods: Mods::PRIMARY,
            }],
        );
}

/// Prompts for a file and renders every frame of the timeline into
/// it, in the format its extension names.
fn export_timeline(world: &mut World) {
    let state = world.resource::<EditorState>();
    let Some(timeline) = state.timeline else {
        return;
    };
    let step = world.resource::<ProjectSettings>().timestep();
    let frames = 0..frame_count(state.duration, step);
    if frames.is_empty() {
        error!(
            "the timeline is empty, so there is nothing to export"
        );
        return;
    }
    let Some(path) = ask_for_path() else {
        return;
    };
    let output = match output_for(path) {
        Ok(output) => output,
        Err(err) => {
            error!("{err}");
            return;
        }
    };
    let target = world.resource::<PreviewImage>().0.clone();
    world.spawn(Export::new(target, timeline, step, frames, output));
}

/// Where an export is written. `None` when the dialog was dismissed.
fn ask_for_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("MP4 video (H.264)", &["mp4"])
        .add_filter("MOV video (ProRes 4444)", &["mov"])
        .add_filter("WebM video (VP9)", &["webm"])
        .add_filter("PNG sequence", &["png"])
        .save_file()
}

/// The output for a file at `path`, by its extension. A file named
/// without one is an MP4.
fn output_for(path: PathBuf) -> Result<Output, String> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase());
    let codec = match extension.as_deref() {
        None | Some("mp4") => Codec::H264,
        Some("mov") => Codec::ProRes4444,
        Some("webm") => Codec::Vp9,
        Some("png") => return Ok(images(path)),
        Some(other) => {
            return Err(format!("cannot export to a .{other} file"));
        }
    };
    let ffmpeg = find_ffmpeg().ok_or(
        "a video needs ffmpeg to encode it, and none was found",
    )?;
    Ok(Output::Video {
        ffmpeg,
        codec,
        path: path.with_extension(codec.extension()),
    })
}

/// A PNG a frame, in a folder beside `path` named after it.
fn images(path: PathBuf) -> Output {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    Output::Images {
        dir: path.with_file_name(format!("{stem}_frames")),
        stem,
    }
}

fn on_finished(finished: On<ExportFinished>) {
    match &finished.result {
        Ok(()) => info!("export finished"),
        Err(err) => error!("export failed: {err}"),
    }
}
