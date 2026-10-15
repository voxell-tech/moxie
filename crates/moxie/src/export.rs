//! Rendering the timeline to a video or a PNG sequence.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use moxie_export::{
    Codec, Export, ExportFinished, Output, Quality, find_ffmpeg,
    frame_count,
};

use crate::{
    EditorState, PreviewImage, ProjectPath, ProjectSettings,
};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(moxie_export::plugin)
        .init_resource::<ExportSettings>()
        .init_resource::<ExportStatus>()
        .insert_resource(Ffmpeg(find_ffmpeg()))
        .add_observer(on_finished);
}

/// What an export is written as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Format {
    #[default]
    Mp4,
    Mov,
    WebM,
    Png,
}

impl Format {
    pub(crate) const ALL: [Self; 4] =
        [Self::Mp4, Self::Mov, Self::WebM, Self::Png];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4 video (H.264)",
            Self::Mov => "MOV video (ProRes 4444)",
            Self::WebM => "WebM video (VP9)",
            Self::Png => "PNG sequence",
        }
    }

    /// The codec ffmpeg encodes it with. `None` for images, which
    /// need no ffmpeg.
    pub(crate) fn codec(self) -> Option<Codec> {
        match self {
            Self::Mp4 => Some(Codec::H264),
            Self::Mov => Some(Codec::ProRes4444),
            Self::WebM => Some(Codec::Vp9),
            Self::Png => None,
        }
    }

    /// Whether [`Quality`] changes what it writes.
    pub(crate) fn has_quality(self) -> bool {
        matches!(self, Self::Mp4 | Self::WebM)
    }

    fn extension(self) -> &'static str {
        self.codec().map_or("png", Codec::extension)
    }
}

/// The choices the export panel shows.
#[derive(Resource)]
pub(crate) struct ExportSettings {
    pub(crate) format: Format,
    pub(crate) quality: Quality,
    /// Every frame of the timeline, or `first..=last` of it.
    pub(crate) whole: bool,
    pub(crate) first: u32,
    pub(crate) last: u32,
    pub(crate) file: Option<PathBuf>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: Format::default(),
            quality: Quality::default(),
            whole: true,
            first: 0,
            last: 0,
            file: None,
        }
    }
}

/// The `ffmpeg` videos are encoded with, if one was found.
#[derive(Resource)]
pub(crate) struct Ffmpeg(pub(crate) Option<PathBuf>);

/// The export running, or how the last one ended.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) enum ExportStatus {
    #[default]
    Idle,
    /// Writing `file`, a folder for a sequence.
    Running {
        job: Entity,
        file: PathBuf,
    },
    Done(PathBuf),
    Failed(String),
    Cancelled,
}

/// The frames the timeline has at the project's frame rate.
pub(crate) fn timeline_frames(world: &World) -> u32 {
    let duration = world.resource::<EditorState>().duration;
    frame_count(
        duration,
        world.resource::<ProjectSettings>().timestep(),
    )
}

/// Looks for ffmpeg again, as one may have been installed since.
pub(crate) fn refind_ffmpeg(world: &mut World) {
    world.insert_resource(Ffmpeg(find_ffmpeg()));
}

/// Sets the format, and the extension of the file picked to match.
pub(crate) fn set_format(world: &mut World, format: Format) {
    let mut settings = world.resource_mut::<ExportSettings>();
    settings.format = format;
    if let Some(file) = settings.file.take() {
        settings.file = Some(file.with_extension(format.extension()));
    }
}

/// Prompts for the file to export to. Whether one was picked.
pub(crate) fn choose_file(world: &mut World) -> bool {
    let settings = world.resource::<ExportSettings>();
    let format = settings.format;
    // The project's own name and folder, until a file is picked.
    let start = settings.file.clone().or_else(|| {
        let project = world.resource::<ProjectPath>().0.clone()?;
        Some(project.with_extension(format.extension()))
    });
    let mut dialog = rfd::FileDialog::new()
        .add_filter(format.label(), &[format.extension()]);
    if let Some(start) = &start {
        if let Some(folder) = start.parent() {
            dialog = dialog.set_directory(folder);
        }
        if let Some(name) = start.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
    }
    let Some(file) = dialog.save_file() else {
        return false;
    };
    world.resource_mut::<ExportSettings>().file =
        Some(file.with_extension(format.extension()));
    true
}

/// Starts exporting with the panel's settings, asking for a file
/// first if none is picked yet.
pub(crate) fn start(world: &mut World) {
    if matches!(
        *world.resource::<ExportStatus>(),
        ExportStatus::Running { .. }
    ) {
        return;
    }
    if world.resource::<ExportSettings>().file.is_none()
        && !choose_file(world)
    {
        return;
    }
    let status = match job(world) {
        Ok((export, file)) => {
            let job = world.spawn(export).id();
            ExportStatus::Running { job, file }
        }
        Err(error) => ExportStatus::Failed(error),
    };
    world.insert_resource(status);
}

/// The export the panel's settings ask for, and the file or folder it
/// writes.
fn job(world: &World) -> Result<(Export, PathBuf), String> {
    let timeline = world
        .resource::<EditorState>()
        .timeline
        .ok_or("there is no timeline to export")?;
    let count = timeline_frames(world);
    if count == 0 {
        return Err("the timeline is empty".into());
    }
    let settings = world.resource::<ExportSettings>();
    let frames = if settings.whole {
        0..count
    } else {
        let last = settings.last.min(count - 1);
        settings.first.min(last)..last + 1
    };
    let file = settings
        .file
        .clone()
        .ok_or("no file was picked to export to")?;
    let (output, written) = match settings.format.codec() {
        Some(codec) => {
            let ffmpeg = world.resource::<Ffmpeg>().0.clone().ok_or(
                "a video needs ffmpeg to encode it, and none was \
                 found",
            )?;
            let output = Output::Video {
                ffmpeg,
                codec,
                quality: settings.quality,
                path: file.clone(),
            };
            (output, file)
        }
        None => {
            let stem = file
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let dir = file.with_file_name(format!("{stem}_frames"));
            let output = Output::Images {
                dir: dir.clone(),
                stem,
            };
            (output, dir)
        }
    };
    let export = Export::new(
        world.resource::<PreviewImage>().0.clone(),
        timeline,
        world.resource::<ProjectSettings>().timestep(),
        frames,
        output,
    );
    Ok((export, written))
}

/// Stops the export running. A video it was writing is deleted; the
/// frames of a sequence written so far are kept.
pub(crate) fn cancel(world: &mut World) {
    if let ExportStatus::Running { job, .. } =
        *world.resource::<ExportStatus>()
    {
        world.despawn(job);
        world.insert_resource(ExportStatus::Cancelled);
    }
}

/// Shows `path` in the system's file browser.
pub(crate) fn reveal(path: &Path) {
    let mut command;
    if cfg!(target_os = "macos") {
        command = std::process::Command::new("open");
        command.arg("-R").arg(path);
    } else if cfg!(windows) {
        let mut select = std::ffi::OsString::from("/select,");
        select.push(path);
        command = std::process::Command::new("explorer");
        command.arg(select);
    } else {
        // No common way to select a file, so its folder is opened.
        command = std::process::Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
    }
    if let Err(err) = command.spawn() {
        error!("could not show {}: {err}", path.display());
    }
}

fn on_finished(
    finished: On<ExportFinished>,
    mut status: ResMut<ExportStatus>,
) {
    let ExportStatus::Running { job, file } = &*status else {
        return;
    };
    if *job != finished.entity {
        return;
    }
    *status = match &finished.result {
        Ok(()) => ExportStatus::Done(file.clone()),
        Err(error) => ExportStatus::Failed(error.clone()),
    };
}
