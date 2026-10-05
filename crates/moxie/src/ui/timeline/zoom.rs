//! Horizontal zoom for the timeline.

use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::events::{Pointer, Scroll};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy_fynix::Theme;
use bevy_fynix::scroll::wheel;
use bevy_fynix::shortcut::Mods;
use moxie_ui::cursor::PointerEventExt as _;
use moxie_ui::theme::EditorTheme;

use super::TrackViewport;
use crate::playback::x_from_cursor;
use crate::{EditorState, ProjectSettings, TimelineView};

/// Zoom factor per wheel notch.
const WHEEL_STEP: f32 = 1.1;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ZoomGoal>()
        .init_resource::<WheelLeft>()
        .add_systems(Update, (ease_zoom, ease_wheel))
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

/// What is left of a wheel notch's pan, along x, and of its scroll
/// through the tracks, along y: a notch is a jump, eased off a share
/// a frame.
#[derive(Resource, Default)]
pub(super) struct WheelLeft(Vec2);

impl WheelLeft {
    /// Takes a wheel's `turn`: to ease off when it came in notches.
    /// A trackpad's steps are small and carry momentum of their own,
    /// so they are returned to follow at once.
    fn take(&mut self, turn: Vec2, notched: bool) -> Vec2 {
        if notched {
            self.0 += turn;
            Vec2::ZERO
        } else {
            self.0 = Vec2::ZERO;
            turn
        }
    }
}

/// Pans the view by `by.x` and scrolls the tracks by `by.y`.
fn slide(
    by: Vec2,
    view: &mut TimelineView,
    computed: &ComputedNode,
    position: &mut ScrollPosition,
) {
    // Panning goes through the view so the time axis and playhead
    // move with the blocks; `ScrollPosition` only carries y.
    if by.x != 0.0 {
        view.pan_by(by.x);
    }
    if by.y != 0.0 {
        let overflow = ((computed.content_size() - computed.size())
            * computed.inverse_scale_factor())
        .max(Vec2::ZERO);
        // Content that shrank since the last scroll leaves
        // `position` past its end.
        position.y = (position.y.min(overflow.y) - by.y)
            .clamp(0.0, overflow.y);
    }
}

/// Eases off what [`WheelLeft`] holds.
fn ease_wheel(
    time: Res<Time>,
    theme: Res<Theme<EditorTheme>>,
    mut left: ResMut<WheelLeft>,
    mut view: ResMut<TimelineView>,
    mut q_viewport: Query<
        (&ComputedNode, &mut ScrollPosition),
        With<TrackViewport>,
    >,
) {
    if left.0 == Vec2::ZERO {
        return;
    }
    let Some((computed, mut position)) = q_viewport.iter_mut().next()
    else {
        left.0 = Vec2::ZERO;
        return;
    };
    let share = theme.0.motion.follow_share(time.delta());
    let step = if left.0.abs().max_element() < 0.5 {
        left.0
    } else {
        left.0 * share
    };
    left.0 -= step;
    slide(step, view.as_mut(), computed, &mut position);
}

/// Zoom on a wheel over the time axis, about the cursor. A sideways
/// wheel still pans, and Shift+wheel is left to the track.
pub(super) fn on_axis_scroll(
    mut scroll: On<Pointer<Scroll>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_scale: Res<UiScale>,
    settings: Res<ProjectSettings>,
    mut goal: ResMut<ZoomGoal>,
    mut left: ResMut<WheelLeft>,
    mut view: ResMut<TimelineView>,
    q_axis: Query<(&ComputedNode, &UiGlobalTransform)>,
) {
    if Mods::held(&keys).has(Mods::SHIFT) {
        return;
    }
    // Or the track scrolls its rows with the same wheel.
    scroll.propagate(false);

    let Ok((computed, transform)) = q_axis.get(scroll.entity) else {
        return;
    };
    let (delta, notched) = wheel(&scroll);

    let pan = left.take(Vec2::X * delta.x, notched);
    if pan.x != 0.0 {
        view.pan_by(pan.x);
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
    mut left: ResMut<WheelLeft>,
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

    let (delta, notched) = wheel(&scroll);

    let mods = Mods::held(&keys);
    if mods.has(Mods::ALT) {
        let cursor = scroll.logical(&ui_scale);
        let anchor_x = x_from_cursor(cursor, computed, transform);
        goal.aim(&view, &settings, delta.y, anchor_x);
        return;
    }

    // Normalize Shift+wheel into horizontal scrolling across
    // platforms.
    let turn = if mods.has(Mods::SHIFT) {
        Vec2::X * if delta.x != 0.0 { delta.x } else { delta.y }
    } else {
        delta
    };
    slide(
        left.take(turn, notched),
        view.as_mut(),
        computed,
        &mut position,
    );
}
