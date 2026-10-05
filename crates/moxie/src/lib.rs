#![doc = include_str!("../README.md")]
#![allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "Inherent to Bevy ECS: systems take many params and \
              query tuples."
)]

mod catalog;
mod icons;
mod keymap;
mod layout;
mod materials;
mod playback;
mod presets;
mod project;
mod scene;
mod subject;
#[cfg(test)]
mod tests;
mod thumbnails;
mod ui;
mod view;

use core::time::Duration;
use std::path::PathBuf;

use bevy::app::PluginGroupBuilder;
use bevy::asset::UnapprovedPathMode;
use bevy::prelude::*;
use bevy_motiongfx::BevyMotionGfxPlugin;
use bevy_motiongfx::prelude::TimelineId;
use bevy_motiongfx::scene::id::EntityUid;
pub use keymap::{KeyOverride, KeymapSettings, settings_plugin};
pub use layout::{LayoutNode, ProjectLayout};
use moxie_asset::{MoxieAssetPlugin, register_absolute_source};
pub(crate) use moxie_ui::SelectedEntity;
pub use project::open_path;
pub use scene::EditorScene;

/// Bevy's [`DefaultPlugins`], with assets set up the way the editor
/// loads them.
pub fn default_plugins() -> PluginGroupBuilder {
    DefaultPlugins
        .build()
        // Asset sources build when `AssetPlugin` does.
        .add_before::<AssetPlugin>(register_absolute_source)
        .set(AssetPlugin {
            file_path: "../../assets".into(),
            unapproved_path_mode: UnapprovedPathMode::Deny,
            ..default()
        })
}

/// Plugin that renders a timeline editor UI for the first
/// [`Timeline`](bevy_motiongfx::prelude::BevyTimeline).
pub struct MoxiePlugin;

impl Plugin for MoxiePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            BevyMotionGfxPlugin,
            MoxieAssetPlugin,
            presets::plugin,
            ui::UiPlugin,
            thumbnails::plugin,
            catalog::plugin,
            materials::plugin,
            moxie_viewport::plugin,
            keymap::plugin,
        ))
        .init_resource::<ProjectSettings>()
        .init_resource::<ProjectLayout>()
        // Ahead of `Startup`, where an app opens the project it was
        // asked for.
        .add_systems(PreStartup, project::new_scene)
        .add_systems(PreUpdate, ensure_scene_root);
    }
}

/// The settings of a project, saved with it.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default, Clone)]
pub struct ProjectSettings {
    /// The resolution the project renders at, in pixels.
    pub size: UVec2,
    /// The shortest an action runs, and the step retiming moves in.
    pub timestep: TimeStep,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            size: UVec2::new(1920, 1080),
            timestep: TimeStep::default(),
        }
    }
}

/// The step a project's time moves in: a frame of a video, or a
/// duration of its own for a project played in real time.
#[derive(Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Default, Clone)]
pub enum TimeStep {
    Fps24,
    Fps25,
    Fps30,
    Fps50,
    Fps60,
    Fps120,
    Custom(Duration),
}

impl Default for TimeStep {
    fn default() -> Self {
        Self::Custom(Duration::from_millis(10))
    }
}

impl TimeStep {
    /// The frames a second it stands for. `None` for a custom step.
    pub fn fps(self) -> Option<u32> {
        match self {
            Self::Fps24 => Some(24),
            Self::Fps25 => Some(25),
            Self::Fps30 => Some(30),
            Self::Fps50 => Some(50),
            Self::Fps60 => Some(60),
            Self::Fps120 => Some(120),
            Self::Custom(_) => None,
        }
    }

    /// How long one step lasts, a frame to the nearest nanosecond.
    pub const fn duration(self) -> Duration {
        match self {
            Self::Fps24 => Duration::from_nanos(41_666_667),
            Self::Fps25 => Duration::from_millis(40),
            Self::Fps30 => Duration::from_nanos(33_333_333),
            Self::Fps50 => Duration::from_millis(20),
            Self::Fps60 => Duration::from_nanos(16_666_667),
            Self::Fps120 => Duration::from_nanos(8_333_333),
            Self::Custom(step) => step,
        }
    }
}

impl ProjectSettings {
    pub(crate) fn timestep(&self) -> Duration {
        // A custom step can be typed down to nothing.
        self.timestep.duration().max(Duration::from_millis(1))
    }

    /// The longest a side of the output is, in pixels.
    const MAX_SIDE: u32 = 8192;

    /// The resolution as it can be rendered: at least a pixel each
    /// way, and no side past [`Self::MAX_SIDE`].
    pub(crate) fn size(&self) -> UVec2 {
        // An inspector can type any number, and neither a zero nor a
        // texture larger than the GPU holds is survived.
        self.size.clamp(UVec2::ONE, UVec2::splat(Self::MAX_SIDE))
    }
}

/// Ensures an [`Entity`] with [`SceneRoot`] exists.
pub(crate) fn ensure_scene_root(
    mut commands: Commands,
    roots: Query<Entity, With<SceneRoot>>,
    root_subjects: Query<Entity, (With<EntityUid>, Without<ChildOf>)>,
) {
    let root_count = roots.count();
    if root_count > 1 {
        error!("There are more than one root in the scene!");
    } else if root_count == 0 {
        let root = commands
            .spawn((
                SceneRoot,
                Transform::IDENTITY,
                Visibility::Inherited,
            ))
            .id();

        for subject in &root_subjects {
            commands.entity(root).add_child(subject);
        }
    }
}

/// Marker component for the root [`Entity`] of the scene.
/// All subjects with [`EntityUid`] lives under this.
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component, Default, Clone)]
pub struct SceneRoot;

/// The coarsest zoom.
const MIN_PX_PER_SECOND: f32 = 1.0;
/// The width of one timestep at the finest zoom.
const FINEST_STEP_PX: f32 = 48.0;

/// Maps animation time to timeline pixels.
#[derive(Resource, Clone, Copy, PartialEq)]
pub(crate) struct TimelineView {
    px_per_second: f32,
    /// Where the timeline's left edge sits.
    offset: Duration,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            px_per_second: 160.0,
            offset: Duration::ZERO,
        }
    }
}

impl TimelineView {
    /// One pixel per second, unpanned.
    pub(crate) const UNIT: Self = Self {
        px_per_second: 1.0,
        offset: Duration::ZERO,
    };

    /// Horizontal pixel offset for a point `t` into the timeline.
    #[inline]
    pub(crate) fn x_from_time(&self, t: Duration) -> f32 {
        let secs = if t >= self.offset {
            (t - self.offset).as_secs_f32()
        } else {
            -(self.offset - t).as_secs_f32()
        };
        secs * self.px_per_second
    }

    /// Seconds spanned by a horizontal distance of `dx`. Unlike
    /// [`time_from_x`](Self::time_from_x) this is a plain scale, for
    /// a drag's delta rather than a position.
    #[inline]
    pub(crate) fn secs_from_dx(&self, dx: f32) -> f32 {
        dx / self.px_per_second
    }

    /// Point into the timeline at `x`, clamped to a non-negative
    /// time.
    #[inline]
    pub(crate) fn time_from_x(&self, x: f32) -> Duration {
        let secs = x / self.px_per_second;
        if !secs.is_finite() {
            return self.offset;
        }
        let step = Duration::from_secs_f32(secs.abs());
        if secs >= 0.0 {
            self.offset.saturating_add(step)
        } else {
            self.offset.saturating_sub(step)
        }
    }

    /// The zoom range of a project that moves in steps of `timestep`:
    /// at the finest, one step is [`FINEST_STEP_PX`] wide.
    pub(crate) fn range(timestep: Duration) -> (f32, f32) {
        let finest = FINEST_STEP_PX / timestep.as_secs_f32();
        (MIN_PX_PER_SECOND, finest.max(MIN_PX_PER_SECOND))
    }

    /// Scale the zoom by `factor` and leave `anchor_time` sitting at
    /// `anchor_x`, saturating at the ends of the range for a project
    /// that moves in steps of `timestep`.
    pub(crate) fn zoom_to(
        &mut self,
        anchor_x: f32,
        anchor_time: Duration,
        factor: f32,
        timestep: Duration,
    ) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let (coarsest, finest) = Self::range(timestep);
        self.px_per_second =
            (self.px_per_second * factor).clamp(coarsest, finest);
        // Put the anchor at the left edge, then push it back to
        // `anchor_x`.
        self.offset = anchor_time;
        self.pan_by(anchor_x);
    }

    /// Slide the view `delta_x` pixels along the timeline, stopping
    /// at the start.
    pub(crate) fn pan_by(&mut self, delta_x: f32) {
        self.offset = self.time_from_x(-delta_x);
    }

    /// Scale the view so a `duration` long animation spans a `width`
    /// px panel, leaving a little room after it, within the range
    /// for a project that moves in steps of `timestep`.
    pub(crate) fn fit(
        &mut self,
        width: f32,
        duration: Duration,
        timestep: Duration,
    ) {
        let secs = duration.as_secs_f32();
        if secs <= 0.0 {
            return;
        }
        let (coarsest, finest) = Self::range(timestep);
        self.px_per_second =
            (width / (secs * 1.02)).clamp(coarsest, finest);
        self.offset = Duration::ZERO;
    }
}

/// The offscreen texture the scene cameras render into, and the
/// preview shows. As large as [`ProjectSettings::size`].
#[derive(Resource)]
pub(crate) struct PreviewImage(pub(crate) Handle<Image>);

/// The focused timeline and its duration.
#[derive(Resource, Default)]
pub(crate) struct EditorState {
    pub(crate) timeline: Option<TimelineId>,
    pub(crate) duration: Duration,
    /// Mirrored from the first
    /// [`RealtimePlayer`](bevy_motiongfx::prelude::RealtimePlayer)
    /// so the play/pause label can bind to this resource instead of
    /// polling a component query.
    pub(crate) is_playing: bool,
}

/// The path (root-to-node child indices) of the action currently
/// selected in the timeline panel, if any. `None` selects nothing.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct SelectedAction(pub(crate) Option<Vec<usize>>);

/// Folders bookmarked for browsing in the asset panel. Saved and
/// loaded with the project: a bookmark only means something alongside
/// the assets it points at.
#[derive(Resource, Default, Clone)]
pub(crate) struct ProjectBookmarks(pub(crate) Vec<PathBuf>);

/// Where the open project's own `.mox` was last loaded from or saved
/// to. Its folder is the asset panel's own, permanent bookmark.
#[derive(Resource, Default, Clone)]
pub(crate) struct ProjectPath(pub(crate) Option<PathBuf>);
