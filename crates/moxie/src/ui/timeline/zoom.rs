//! Horizontal zoom for the timeline.

use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::events::{Pointer, Scroll};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy_fynix::Theme;
use moxie_ui::cursor::PointerEventExt as _;
use moxie_ui::theme::EditorTheme;

use super::TrackViewport;
use super::reorder::scrolled;
use crate::playback::x_from_cursor;
use crate::{EditorState, ProjectSettings, TimelineView};

/// Zoom factor per wheel notch.
const WHEEL_STEP: f32 = 1.1;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ZoomGoal>()
        .add_systems(Update, ease_zoom)
        .add_observer(on_fit_timeline);
}

/// The zoom the timeline is easing to, in pixels a second, and the
/// point it zooms about, in pixels from the left edge.
#[derive(Resource, Default)]
pub(super) struct ZoomGoal(Option<(f32, f32)>);

impl ZoomGoal {
    /// Sets the goal `wheel` pixels of wheel on from where the zoom
    /// is already headed, so quick notches add up.
    fn aim(
        &mut self,
        view: &TimelineView,
        settings: &ProjectSettings,
        wheel: f32,
        anchor_x: f32,
    ) {
        let notches =
            wheel / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR;
        let from = self.0.map_or(view.px_per_second, |(to, _)| to);
        let (coarsest, finest) =
            TimelineView::range(settings.timestep());
        let to =
            (from * WHEEL_STEP.powf(notches)).clamp(coarsest, finest);
        self.0 = Some((to, anchor_x));
    }
}

/// Eases the zoom on to its [`ZoomGoal`].
fn ease_zoom(
    time: Res<Time>,
    theme: Res<Theme<EditorTheme>>,
    settings: Res<ProjectSettings>,
    mut goal: ResMut<ZoomGoal>,
    mut view: ResMut<TimelineView>,
) {
    let Some((to, anchor_x)) = goal.0 else {
        return;
    };
    let now = view.px_per_second;
    // In ratios: a zoom is as far from half as it is from double.
    let share = theme.0.motion.follow_share(time.delta());
    let eased = now * (to / now).powf(share);
    let arrived = (eased / to - 1.0).abs() < 1e-3;
    let next = if arrived { to } else { eased };
    let anchor_time = view.time_from_x(anchor_x);
    view.zoom_to(
        anchor_x,
        anchor_time,
        next / now,
        settings.timestep(),
    );
    if arrived {
        goal.0 = None;
    }
}

/// Command to fit the animation to the panel, dispatched from the fit
/// button and handled in [`on_fit_timeline`].
#[derive(Event)]
pub(super) struct FitTimeline;

/// Scale the view so the animation spans the track viewport.
fn on_fit_timeline(
    _fit: On<FitTimeline>,
    q_viewport: Query<&ComputedNode, With<TrackViewport>>,
    state: Res<EditorState>,
    settings: Res<ProjectSettings>,
    mut goal: ResMut<ZoomGoal>,
    mut view: ResMut<TimelineView>,
) {
    let Some(computed) = q_viewport.iter().next() else {
        return;
    };
    let width = computed.size().x * computed.inverse_scale_factor();
    goal.0 = None;
    view.fit(width, state.duration, settings.timestep());
}

/// Zoom on a wheel over the time axis, about the cursor. A sideways
/// wheel still pans, and Shift+wheel is left to the track.
pub(super) fn on_axis_scroll(
    mut scroll: On<Pointer<Scroll>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_scale: Res<UiScale>,
    settings: Res<ProjectSettings>,
    mut goal: ResMut<ZoomGoal>,
    mut view: ResMut<TimelineView>,
    q_axis: Query<(&ComputedNode, &UiGlobalTransform)>,
) {
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        return;
    }
    // Or the track scrolls its rows with the same wheel.
    scroll.propagate(false);

    let Ok((computed, transform)) = q_axis.get(scroll.entity) else {
        return;
    };
    let delta = Vec2::new(scroll.x, scroll.y)
        * match scroll.unit {
            MouseScrollUnit::Line => {
                MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR
            }
            MouseScrollUnit::Pixel => 1.0,
        };

    if delta.x != 0.0 {
        view.pan_by(delta.x);
    }
    if delta.y != 0.0 {
        let cursor = scroll.logical(&ui_scale);
        let anchor_x = x_from_cursor(cursor, computed, transform);
        goal.aim(&view, &settings, delta.y, anchor_x);
    }
}

/// Zoom on Alt+wheel, pan sideways on Shift+wheel or a
/// horizontal wheel, and scroll the tracks otherwise.
pub(super) fn on_track_scroll(
    mut scroll: On<Pointer<Scroll>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_scale: Res<UiScale>,
    settings: Res<ProjectSettings>,
    mut goal: ResMut<ZoomGoal>,
    mut view: ResMut<TimelineView>,
    mut q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<TrackViewport>,
    >,
) {
    scroll.propagate(false);

    let Some((computed, transform, mut position)) =
        q_viewport.iter_mut().next()
    else {
        return;
    };

    let px_per_notch = MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR;
    let delta = Vec2::new(scroll.x, scroll.y)
        * match scroll.unit {
            MouseScrollUnit::Line => px_per_notch,
            MouseScrollUnit::Pixel => 1.0,
        };

    if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        let cursor = scroll.logical(&ui_scale);
        let anchor_x = x_from_cursor(cursor, computed, transform);
        goal.aim(&view, &settings, delta.y, anchor_x);
        return;
    }

    // Normalize Shift+wheel into horizontal scrolling across
    // platforms.
    let sideways =
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let (pan_x, scroll_y) = if sideways {
        (if delta.x != 0.0 { delta.x } else { delta.y }, 0.0)
    } else {
        (delta.x, delta.y)
    };

    // Panning goes through the view so the time axis and playhead
    // move with the blocks; `ScrollPosition` only carries y.
    if pan_x != 0.0 {
        view.pan_by(pan_x);
    }

    if scroll_y != 0.0 {
        let inv = computed.inverse_scale_factor();
        let overflow = ((computed.content_size() - computed.size())
            * inv)
            .max(Vec2::ZERO);
        // From where the layout has it: content that shrank since the
        // last scroll leaves `position` past its end.
        position.y =
            (scrolled(computed) - scroll_y).clamp(0.0, overflow.y);
    }
}
