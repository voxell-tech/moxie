//! Renders a timeline frame by frame into image files or a video.
//!
//! An [`Export`] steps its timeline one frame at a time, captures
//! what the cameras draw into its target, and hands the frames to its
//! [`Output`] in order. Time comes from the frame's number alone, so
//! the result is the same however fast the frames render.

mod images;
mod video;

use core::fmt::Display;
use core::ops::Range;
use core::time::Duration;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{
    Screenshot, ScreenshotCaptured,
};
use bevy_motiongfx::prelude::{
    MotionGfxManager, RealtimePlayer, TimelineId,
};
pub use video::{Codec, find_ffmpeg};

/// Frames rendered and not yet written, at most.
const MAX_IN_FLIGHT: u32 = 4;
/// Frames drawn before the first is kept: a camera or a mesh new
/// this frame is not drawn until a frame or two later.
const WARM_UP: u8 = 3;

/// Runs every [`Export`], one at a time.
pub fn plugin(app: &mut App) {
    app.add_systems(Update, drive)
        .add_observer(on_captured)
        .add_observer(on_removed);
}

/// Where an [`Export`] writes its frames.
pub enum Output {
    /// A PNG a frame, `<stem>_<frame>.png` in `dir`.
    Images { dir: PathBuf, stem: String },
    /// One video file at `path`, encoded by the `ffmpeg` executable
    /// given.
    Video {
        ffmpeg: PathBuf,
        codec: Codec,
        path: PathBuf,
    },
}

/// A render of a timeline to files. Lives on an entity of its own
/// until it ends, which [`ExportFinished`] tells. Despawning it
/// cancels it.
#[derive(Component)]
pub struct Export {
    target: Handle<Image>,
    timeline: TimelineId,
    step: Duration,
    frames: Range<u32>,
    /// The frame rendered next.
    next: u32,
    /// The frame the sink takes next.
    due: u32,
    /// Frames the sink has finished with.
    written: u32,
    /// Frames captured ahead of their turn.
    captured: BTreeMap<u32, Frame>,
    /// Frames still to draw at the first frame's time before any is
    /// kept.
    warm_up: u8,
    sink: Sink,
    /// The time the timeline is put back to.
    resume: Option<Duration>,
    failure: Option<String>,
}

impl Export {
    /// An export of `frames` of `timeline`, a `step` of its time
    /// apart, as its cameras draw them into `target`.
    pub fn new(
        target: Handle<Image>,
        timeline: TimelineId,
        step: Duration,
        frames: Range<u32>,
        output: Output,
    ) -> Self {
        let sink = match output {
            Output::Images { dir, stem } => {
                Sink::Images(images::Images::new(dir, stem))
            }
            Output::Video {
                ffmpeg,
                codec,
                path,
            } => Sink::Video(video::Video::new(
                ffmpeg, codec, path, step,
            )),
        };
        Self {
            target,
            timeline,
            step,
            next: frames.start,
            due: frames.start,
            frames,
            written: 0,
            captured: BTreeMap::new(),
            warm_up: WARM_UP,
            sink,
            resume: None,
            failure: None,
        }
    }

    /// The frames written so far.
    pub fn written(&self) -> u32 {
        self.written
    }

    /// The frames there are to write.
    pub fn total(&self) -> u32 {
        self.frames.len() as u32
    }

    /// Moves the export along. `Ok(None)` once it has ended, and
    /// otherwise the frame to render now, if it is time for one.
    fn advance(&mut self) -> Result<Option<Pending>, String> {
        if let Some(failure) = self.failure.take() {
            return Err(failure);
        }
        self.written += self.sink.finished()?;
        while self.sink.ready()
            && let Some(frame) = self.captured.remove(&self.due)
        {
            self.sink.write(self.due, frame)?;
            self.due += 1;
        }
        if self.written == self.total() {
            return Ok(if self.sink.close()? {
                None
            } else {
                Some(Pending::Wait)
            });
        }
        if self.warm_up > 0 {
            self.warm_up -= 1;
            return Ok(Some(Pending::Seek(self.next)));
        }
        let rendered = self.next - self.frames.start;
        if self.next < self.frames.end
            && rendered - self.written < MAX_IN_FLIGHT
        {
            self.next += 1;
            return Ok(Some(Pending::Capture(self.next - 1)));
        }
        Ok(Some(Pending::Wait))
    }
}

/// What an [`Export`] still running needs this frame.
enum Pending {
    /// The timeline at a frame's time, drawn and not kept.
    Seek(u32),
    /// A frame drawn and captured.
    Capture(u32),
    Wait,
}

/// Told on an [`Export`]'s entity as it ends, just before the entity
/// is despawned.
#[derive(EntityEvent)]
pub struct ExportFinished {
    pub entity: Entity,
    pub result: Result<(), String>,
}

/// The frames a timeline `duration` long has at `step` apart.
pub fn frame_count(duration: Duration, step: Duration) -> u32 {
    let step = step.as_nanos().max(1);
    u32::try_from(duration.as_nanos().div_ceil(step))
        .unwrap_or(u32::MAX)
}

fn unwritten(path: &Path, err: impl Display) -> String {
    format!("could not write {}: {err}", path.display())
}

/// One captured frame: tightly packed sRGB RGBA bytes.
struct Frame {
    size: UVec2,
    data: Vec<u8>,
}

enum Sink {
    Images(images::Images),
    Video(video::Video),
}

impl Sink {
    /// Whether [`Self::write`] can take a frame now.
    fn ready(&self) -> bool {
        match self {
            Self::Images(images) => images.ready(),
            Self::Video(video) => video.ready(),
        }
    }

    fn write(
        &mut self,
        index: u32,
        frame: Frame,
    ) -> Result<(), String> {
        match self {
            Self::Images(images) => images.write(index, frame),
            Self::Video(video) => video.write(frame),
        }
    }

    /// The frames finished with since this was last asked.
    fn finished(&mut self) -> Result<u32, String> {
        match self {
            Self::Images(images) => images.finished(),
            Self::Video(video) => video.finished(),
        }
    }

    /// Ends the output, once every frame is written. Whether it has
    /// ended yet.
    fn close(&mut self) -> Result<bool, String> {
        match self {
            Self::Images(_) => Ok(true),
            Self::Video(video) => video.close(),
        }
    }
}

/// On the screenshot of one frame of an [`Export`].
#[derive(Component)]
struct Shot {
    job: Entity,
    index: u32,
}

fn drive(
    mut commands: Commands,
    mut jobs: Query<(Entity, &mut Export)>,
    mut manager: ResMut<MotionGfxManager>,
    mut players: Query<(&TimelineId, &mut RealtimePlayer)>,
) {
    // The others wait: two would fight over the timeline.
    let Some((entity, mut job)) = jobs.iter_mut().next() else {
        return;
    };
    let pending = job.advance().and_then(|pending| {
        let (index, capture) = match pending {
            Some(Pending::Seek(index)) => (index, false),
            Some(Pending::Capture(index)) => (index, true),
            Some(Pending::Wait) => return Ok(true),
            None => return Ok(false),
        };
        let timeline = manager
            .get_timeline_mut(&job.timeline)
            .ok_or("the timeline to export is gone")?;
        job.resume.get_or_insert(timeline.target_time());
        timeline.set_target_track(0);
        timeline.set_target_time(job.step * index);
        for (id, mut player) in &mut players {
            if *id == job.timeline && player.is_playing {
                player.is_playing = false;
            }
        }
        if capture {
            // Captured as this frame is drawn, after the timeline
            // is sampled.
            commands.spawn((
                Screenshot::image(job.target.clone()),
                Shot { job: entity, index },
            ));
        }
        Ok(true)
    });
    let result = match pending {
        Ok(true) => return,
        Ok(false) => Ok(()),
        Err(error) => Err(error),
    };
    commands.trigger(ExportFinished { entity, result });
    commands.entity(entity).despawn();
}

fn on_captured(
    mut captured: On<ScreenshotCaptured>,
    shots: Query<&Shot>,
    mut jobs: Query<&mut Export>,
) {
    let Ok(shot) = shots.get(captured.entity) else {
        return;
    };
    let Ok(mut job) = jobs.get_mut(shot.job) else {
        return;
    };
    let image = &mut captured.image;
    let format = image.texture_descriptor.format;
    if !matches!(
        format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    ) {
        job.failure =
            Some(format!("cannot export a {format:?} image"));
        return;
    }
    let frame = Frame {
        size: image.size(),
        data: image.data.take().unwrap_or_default(),
    };
    job.captured.insert(shot.index, frame);
}

/// Puts the timeline back where it was before the export moved it.
fn on_removed(
    removed: On<Remove, Export>,
    jobs: Query<&Export>,
    mut manager: ResMut<MotionGfxManager>,
) {
    let Ok(job) = jobs.get(removed.entity) else {
        return;
    };
    if let Some(time) = job.resume
        && let Some(timeline) =
            manager.get_timeline_mut(&job.timeline)
    {
        timeline.set_target_time(time);
    }
}
