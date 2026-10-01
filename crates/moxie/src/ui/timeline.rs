//! The timeline panel: control bar (play/pause + time readout) and a
//! scrubbable track viewport, edge to edge. No name gutter: a
//! block's own header box already carries its label.

// STUB: ported in wave 3 step 2. Only the panel's view and the
// interaction modules that build on it (create, hint, landing,
// reorder, retime) are left out; the rest is kept as it was.

mod block_layout;
mod pattern;
mod prune;
mod time_axis;
mod zoom;

use core::time::Duration;
use std::collections::BTreeSet;

use bevy::prelude::*;
use bevy_fynix::{AnyView, Bevy};
use bevy_motiongfx::prelude::MotionGfxManager;
use block_layout::Placed;
use moxie_ui::theme::{EditorTheme, Spacing};

use crate::{EditorScene, EditorState, TimelineView};

/// The timeline's resources and interaction systems.
pub(crate) struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimelineView>()
            .init_resource::<BlockFoldState>()
            .init_resource::<RebuildTick>()
            .add_plugins((pattern::plugin, zoom::plugin));
    }
}

/// Folded blocks, by path.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct BlockFoldState(BTreeSet<Vec<usize>>);

impl BlockFoldState {
    /// The folded paths this holds.
    pub(crate) fn paths(&self) -> &BTreeSet<Vec<usize>> {
        &self.0
    }
}

fn toggle_folded(world: &mut World, path: &[usize]) {
    let mut state = world.resource_mut::<BlockFoldState>();
    if !state.0.remove(path) {
        state.0.insert(path.to_vec());
    }
}

const CONTROL_BAR_HEIGHT: f32 = 40.0;
const TIME_AXIS_HEIGHT: f32 = 24.0;
const MAJOR_TICK: f32 = 8.0;
const MINOR_TICK: f32 = 4.0;

/// Viewport where the timeline, track and action UI is displayed.
#[derive(Component, Default, Clone)]
pub(crate) struct TrackViewport;

/// The timeline panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    // STUB: ported in wave 3 step 2
    super::stub_panel("Timeline")
}

/// The time axis's width and the view it draws, so a change to
/// either retriggers the watch. Width is rounded so sub-pixel
/// jitter cannot.
fn axis_view(world: &World, node: Entity) -> (u32, TimelineView) {
    let width = world
        .get::<ComputedNode>(node)
        .map(|computed| {
            (computed.size().x * computed.inverse_scale_factor())
                as u32
        })
        .unwrap_or(0);

    (width, *world.resource::<TimelineView>())
}

fn current_time(world: &World) -> Duration {
    let state = world.resource::<EditorState>();
    let Some(id) = state.timeline else {
        return Duration::ZERO;
    };
    world
        .resource::<MotionGfxManager>()
        .get_timeline(&id)
        .map(|t| t.target_time())
        .unwrap_or(Duration::ZERO)
}

/// The editor scene's animation tree, laid out as nested boxes. Nested
/// boxes are a percent of their parent, so the layout ignores the view
/// and only the root box follows it.
fn block_placements(world: &World, space: Spacing) -> Vec<Placed> {
    let empty = BTreeSet::new();
    let folded = world
        .get_resource::<BlockFoldState>()
        .map_or(&empty, |state| &state.0);

    world
        .get_resource::<EditorScene>()
        .map(|editor_scene| {
            block_layout::layout(
                &editor_scene.scene().0.animation,
                TimelineView::UNIT,
                folded,
                space,
            )
        })
        .unwrap_or_default()
}

/// Counter bumped by every committed reorder.
#[derive(Resource, Default)]
pub(crate) struct RebuildTick(u64);

impl RebuildTick {
    /// Forces the box list to rebuild.
    pub(crate) fn bump(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }

    /// Bumps the world's tick, if it has one.
    pub(crate) fn bump_in(world: &mut World) {
        if let Some(mut tick) = world.get_resource_mut::<Self>() {
            tick.bump();
        }
    }
}
