//! Playback control: play/pause (button + spacebar), scrubbing, and
//! the playhead / time readout.

use core::time::Duration;

use bevy::input_focus::InputFocus;
use bevy::picking::events::{
    Cancel, Drag, DragEnd, Pointer, Press, Release,
};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::ui_widgets::ValueChange;
use bevy_motiongfx::prelude::*;

use crate::{EditorState, TimelineView};
use bevy_motiongfx::prelude::TimelineId;

/// Command to flip playback, dispatched from the play/pause button
/// and the spacebar hotkey and handled in one place
/// ([`on_toggle_playback`]).
#[derive(Event)]
pub(crate) struct TogglePlayback;

/// Request a toggle when the spacebar is pressed.
pub(crate) fn play_pause_hotkey(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Space) {
        commands.trigger(TogglePlayback);
    }
}

/// Flip `is_playing` for all players, rewinding to the start first if
/// playback is starting from the end of the track.
pub(crate) fn on_toggle_playback(
    _toggle: On<TogglePlayback>,
    state: Res<EditorState>,
    mut manager: ResMut<MotionGfxManager>,
    mut q_players: Query<&mut RealtimePlayer>,
) {
    // One global target: invert the aggregate, not each player, so
    // mixed states resolve to a single play/pause, not a swap.
    let should_play = !q_players.iter().any(|p| p.is_playing);

    // A zero length track has nothing to play.
    if should_play && state.duration == Duration::ZERO {
        return;
    }

    for mut player in &mut q_players {
        player.is_playing = should_play;
        player.time_scale = 1.0;
    }

    // Rewind if starting playback from the very end.
    if let Some(timeline_id) = state.timeline
        && should_play
        && let Some(timeline) = manager.get_timeline_mut(&timeline_id)
        && timeline.target_time() >= state.duration
    {
        timeline.set_target_track(0);
        timeline.set_target_time(Duration::ZERO);
    }
}

/// Keep [`EditorState`] tracking the first timeline.
///
/// A system, not a binding: the write lands on a resource, which
/// belongs to no node. It writes only when the answer moves, so a
/// change-detecting reader still sees one change per change.
pub(crate) fn track_first_timeline(
    timelines: Query<&TimelineId>,
    manager: Res<MotionGfxManager>,
    mut state: ResMut<EditorState>,
) {
    let Some(&id) = timelines.iter().next() else {
        return;
    };

    let duration = manager
        .get_timeline(&id)
        .and_then(|timeline| {
            timeline.tracks().first().map(|track| track.duration())
        })
        .unwrap_or(Duration::ZERO);

    if state.timeline != Some(id) || state.duration != duration {
        state.timeline = Some(id);
        state.duration = duration;
    }
}

/// Present on the timeline track while a scrub is in progress. A
/// scrub is only ever started by a [`Pointer<Press>`] on the track
/// itself, so drags that began anywhere else can't move the playhead.
#[derive(Component)]
pub(crate) struct Scrubbing;

/// Pixels from the node's left edge to `cursor`.
pub(crate) fn x_from_cursor(
    cursor: Vec2,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
) -> f32 {
    let inv = computed.inverse_scale_factor();
    let (_scale, _angle, center) =
        transform.to_scale_angle_translation();
    let rect = Rect::from_center_size(
        center.trunc() * inv,
        computed.size() * inv,
    );
    cursor.x - rect.min.x
}

/// Move the timeline to `time` and stop playback so the scrub isn't
/// immediately overwritten by the player.
fn scrub_to(
    time: Duration,
    state: &EditorState,
    manager: &mut MotionGfxManager,
    q_players: &mut Query<&mut RealtimePlayer>,
) {
    let Some(timeline_id) = state.timeline else {
        return;
    };
    for mut player in q_players {
        player.is_playing = false;
    }
    if let Some(timeline) = manager.get_timeline_mut(&timeline_id) {
        timeline.set_target_track(0);
        timeline.set_target_time(time);
    }
}

/// Begin a scrub: jump the playhead to the press position and arm
/// [`Scrubbing`] so subsequent drags keep following the cursor.
///
/// `press.entity` is already exactly the entity this observer is
/// registered on: `bevy_picking`'s propagation rewrites a `Pointer`
/// event's `entity` field to match whichever ancestor's observer is
/// running, so there's nothing here to filter on.
pub(crate) fn on_track_press(
    mut press: On<Pointer<Press>>,
    state: Res<EditorState>,
    ui_scale: Res<UiScale>,
    view: Res<TimelineView>,
    q_track: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut manager: ResMut<MotionGfxManager>,
    mut q_players: Query<&mut RealtimePlayer>,
    mut commands: Commands,
) {
    let track = press.entity;
    let Ok((computed, transform)) = q_track.get(track) else {
        return;
    };
    press.propagate(false);
    commands.entity(track).insert(Scrubbing);

    let cursor = press.pointer_location.position / ui_scale.0;
    let time = view
        .time_from_x(x_from_cursor(cursor, computed, transform))
        .min(state.duration);
    scrub_to(time, &state, &mut manager, &mut q_players);
}

/// Continue an armed scrub. Dragging past either end clamps, and a
/// drag that never pressed the track is ignored.
pub(crate) fn on_track_drag(
    mut drag: On<Pointer<Drag>>,
    state: Res<EditorState>,
    ui_scale: Res<UiScale>,
    view: Res<TimelineView>,
    q_track: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<Scrubbing>,
    >,
    mut manager: ResMut<MotionGfxManager>,
    mut q_players: Query<&mut RealtimePlayer>,
) {
    let Ok((computed, transform)) = q_track.get(drag.entity) else {
        return;
    };
    drag.propagate(false);

    let cursor = drag.pointer_location.position / ui_scale.0;
    let time = view
        .time_from_x(x_from_cursor(cursor, computed, transform))
        .min(state.duration);
    scrub_to(time, &state, &mut manager, &mut q_players);
}

/// End a scrub when a drag finishes off the track.
pub(crate) fn on_track_release(
    release: On<Pointer<DragEnd>>,
    mut commands: Commands,
) {
    commands.entity(release.entity).remove::<Scrubbing>();
}

/// End a scrub on a plain press/release with no intervening drag.
/// [`Pointer<DragEnd>`] never fires for that gesture, so without this
/// a single click would leave [`Scrubbing`] armed.
pub(crate) fn on_track_click_release(
    release: On<Pointer<Release>>,
    mut commands: Commands,
) {
    commands.entity(release.entity).remove::<Scrubbing>();
}

pub(crate) fn on_track_cancel(
    cancel: On<Pointer<Cancel>>,
    mut commands: Commands,
) {
    commands.entity(cancel.entity).remove::<Scrubbing>();
}

/// Clear [`RealtimePlayer::is_playing`] once playback reaches the end
/// of the current track.
///
/// [`Timeline::set_target_time`] clamps to the track's duration, so
/// the player would otherwise keep "playing" against the clamp and
/// the button would stay stuck on "Pause".
///
/// [`Timeline::set_target_time`]: bevy_motiongfx::prelude::Timeline::set_target_time
pub(crate) fn stop_at_track_end(
    state: Res<EditorState>,
    manager: Res<MotionGfxManager>,
    mut q_players: Query<&mut RealtimePlayer>,
) {
    let Some(timeline_id) = state.timeline else {
        return;
    };
    let Some(timeline) = manager.get_timeline(&timeline_id) else {
        return;
    };
    if state.duration == Duration::ZERO {
        return;
    }

    // Playing backwards stops at the start instead.
    for mut player in &mut q_players {
        if !player.is_playing {
            continue;
        }
        let at_end = if player.time_scale >= 0.0 {
            timeline.target_time() >= state.duration
        } else {
            timeline.target_time() == Duration::ZERO
        };
        if at_end {
            player.is_playing = false;
        }
    }
}

/// Keep [`EditorState::is_playing`] tracking the players.
pub(crate) fn track_playing(
    q_players: Query<&RealtimePlayer>,
    mut state: ResMut<EditorState>,
) {
    let is_playing = q_players.iter().any(|player| player.is_playing);
    if state.is_playing != is_playing {
        state.is_playing = is_playing;
    }
}

/// Seek to a time typed into the control bar's readout.
/// Only a finished edit moves the playhead.
pub(crate) fn on_time_entered(
    change: On<ValueChange<f32>>,
    state: Res<EditorState>,
    mut focus: ResMut<InputFocus>,
    mut manager: ResMut<MotionGfxManager>,
    mut q_players: Query<&mut RealtimePlayer>,
) {
    if !change.is_final {
        return;
    }
    focus.clear();
    let secs = change.value.clamp(0.0, state.duration.as_secs_f32());
    let Ok(time) = Duration::try_from_secs_f32(secs) else {
        return;
    };
    scrub_to(time, &state, &mut manager, &mut q_players);
}
