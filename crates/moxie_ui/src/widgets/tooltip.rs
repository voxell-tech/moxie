//! A tooltip: UI that appears near the cursor after a short pause.
//!
//! Attached per-entity with [`TooltipExt`], the same way
//! `draggable_field` wires up a drag: the hover lives in one shared
//! `TooltipState` rather than on the element itself.
//!
//! The tooltip is itself interactable, not just decoration: it stays
//! up while the pointer sits on it, not only on the source, so a link
//! or a button inside one can be reached and clicked. A source inside
//! a tooltip opens a tooltip of its own on top, and each tooltip
//! stays up while the pointer is on its source, on it, or on a
//! tooltip opened from it.

use core::time::Duration;

use bevy::picking::events::{Out, Over, Pointer};
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy_fynix::WorldEntityMut;
use fynix::prelude::*;

use crate::elements::{Frame, Label, MenuSurface};
use crate::reactive::{BevyUi, watch_root};
use crate::theme::EditorTheme;

/// How long the pointer has to rest on the source before the tooltip
/// appears.
const DELAY: Duration = Duration::from_millis(500);

/// How long neither the source nor the tooltip may be hovered before
/// it hides.
const HIDE_GRACE: Duration = Duration::from_millis(150);

/// Where the tooltip sits relative to the cursor it appeared at,
/// clear of the pointer hotspot.
const OFFSET: Vec2 = Vec2::new(12.0, 18.0);

/// Every tooltip on its way to showing or showing, oldest first. A
/// tooltip is always pushed after the tooltip its source sits in.
#[derive(Resource, Default)]
struct TooltipState {
    tooltips: Vec<Tooltip>,
}

/// One tooltip, from the moment its source is hovered.
struct Tooltip {
    /// The tooltip's root. Built hidden as soon as the source is
    /// hovered, and shown once the pointer has rested [`DELAY`].
    root: Entity,
    source: Entity,
    /// The root of the tooltip `source` sits in, if any. At most one
    /// tooltip per parent exists at a time.
    parent: Option<Entity>,
    /// How many tooltips it is stacked on.
    depth: usize,
    revealed: bool,
    /// How long the pointer has rested on `source`, before it shows.
    resting: Duration,
    hovered_source: bool,
    hovered_surface: bool,
    /// Counts up once nothing holding the tooltip open is hovered;
    /// `None` while something is.
    hiding: Option<Duration>,
}

impl TooltipState {
    /// Drops the tooltip at `root`, and every tooltip opened from it.
    fn drop_tooltip(
        &mut self,
        root: Entity,
        commands: &mut Commands,
    ) {
        let mut doomed = vec![root];
        let mut at = 0;
        while let Some(&parent) = doomed.get(at) {
            doomed.extend(
                self.tooltips
                    .iter()
                    .filter(|tooltip| tooltip.parent == Some(parent))
                    .map(|tooltip| tooltip.root),
            );
            at += 1;
        }

        self.tooltips.retain(|tooltip| {
            let keep = !doomed.contains(&tooltip.root);
            if !keep {
                commands.entity(tooltip.root).despawn();
            }
            keep
        });
    }
}

/// A tooltip's own surface, so the tooltip it belongs to can be found
/// from anything inside it.
#[derive(Component)]
struct TooltipMark(Entity);

/// Registers [`tick`], the system [`TooltipExt`] otherwise has
/// nothing to drive it.
pub(crate) struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TooltipState>()
            .add_systems(Update, tick);
    }
}

/// Tooltips for anything that can be observed.
pub trait TooltipExt: WorldEntityMut {
    /// Shows `text` near the cursor after a short hover, and keeps it
    /// up as long as the pointer stays on this element or the
    /// tooltip.
    fn tooltip(&mut self, text: impl Into<String>) -> &mut Self {
        let text = text.into();
        self.tooltip_with(move |ui| {
            let h = ui.theme.space.md;
            let v = ui.theme.space.xs;
            let size = ui.theme.text.small;
            let color = ui.theme.color.text;
            let text = text.clone();

            // Its own room beyond `MenuSurface`'s own padding: that's
            // sized for a menu row, snug on a bare tooltip.
            ui.elem(elem!(
                Frame,
                padding = UiRect::axes(px(h), px(v))
            ))
            .with(move |ui| {
                ui.elem(elem!(
                    Label,
                    text = text,
                    size = size,
                    color = color,
                    wrap = false
                ));
            });
        })
    }

    /// Shows whatever `build` makes inside a [`MenuSurface`] near the
    /// cursor after a short hover, and keeps it up as long as the
    /// pointer stays on this element, the tooltip, or a tooltip
    /// opened from it.
    ///
    /// `build` runs each time the pointer enters this element, so it
    /// should be cheap.
    fn tooltip_with<B>(&mut self, build: B) -> &mut Self
    where
        B: Fn(&mut BevyUi) + Clone + Send + Sync + 'static,
    {
        let source = self.id();
        self.observe(
            move |_: On<Pointer<Over>>,
                  mut state: ResMut<TooltipState>,
                  parents: Query<&ChildOf>,
                  marks: Query<&TooltipMark>,
                  mut commands: Commands| {
                if let Some(tooltip) = state
                    .tooltips
                    .iter_mut()
                    .find(|tooltip| tooltip.source == source)
                {
                    tooltip.hovered_source = true;
                    tooltip.hiding = None;
                    return;
                }

                let parent =
                    enclosing_tooltip(source, &parents, &marks);

                // The pointer moved to another source at this level
                // while its neighbour's tooltip was still up: that
                // tooltip goes.
                let neighbour = state
                    .tooltips
                    .iter()
                    .find(|tooltip| tooltip.parent == parent)
                    .map(|tooltip| tooltip.root);
                if let Some(neighbour) = neighbour {
                    state.drop_tooltip(neighbour, &mut commands);
                }

                let depth = parent
                    .and_then(|parent| {
                        state
                            .tooltips
                            .iter()
                            .find(|tooltip| tooltip.root == parent)
                    })
                    .map_or(0, |tooltip| tooltip.depth + 1);

                // `Pickable::IGNORE`, or this full-screen node blocks
                // the very hover that showed it: it swallows the
                // pointer, the source loses `Over`, the tooltip
                // despawns, hover resumes, and it loops.
                let root = commands
                    .spawn((
                        Node {
                            display: Display::None,
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .id();
                state.tooltips.push(Tooltip {
                    root,
                    source,
                    parent,
                    depth,
                    revealed: false,
                    resting: Duration::ZERO,
                    hovered_source: true,
                    hovered_surface: false,
                    hiding: None,
                });

                let build = build.clone();
                commands.queue(move |world: &mut World| {
                    spawn_tooltip(world, root, depth, build)
                });
            },
        )
        .observe(
            move |_: On<Pointer<Out>>,
                  mut state: ResMut<TooltipState>,
                  mut commands: Commands| {
                let Some(tooltip) = state
                    .tooltips
                    .iter_mut()
                    .find(|tooltip| tooltip.source == source)
                else {
                    return;
                };
                tooltip.hovered_source = false;

                // Never shown, so nothing to hold open.
                if !tooltip.revealed {
                    let root = tooltip.root;
                    state.drop_tooltip(root, &mut commands);
                }
            },
        )
    }
}

impl<T: WorldEntityMut> TooltipExt for T {}

/// The tooltip `entity` sits inside, if any.
fn enclosing_tooltip(
    entity: Entity,
    parents: &Query<&ChildOf>,
    marks: &Query<&TooltipMark>,
) -> Option<Entity> {
    let mut at = entity;
    loop {
        if let Ok(mark) = marks.get(at) {
            return Some(mark.0);
        }
        at = parents.get(at).ok()?.parent();
    }
}

/// Counts each tooltip's show delay, shows it once it elapses, and
/// counts the hide grace once nothing holding a shown tooltip open is
/// hovered any longer.
fn tick(
    time: Res<Time>,
    scale: Res<UiScale>,
    pointers: Query<&PointerLocation>,
    marks: Query<(Entity, &TooltipMark)>,
    mut nodes: Query<&mut Node>,
    mut state: ResMut<TooltipState>,
    mut commands: Commands,
) {
    let state = &mut *state;
    let delta = time.delta();
    let cursor = pointers
        .iter()
        .find_map(|pointer| pointer.location())
        .map(|location| location.position / scale.0);

    // A tooltip is held open by its source, by itself, or by a
    // tooltip opened from it. Deepest first: a tooltip opened from
    // another comes after it.
    let mut held = vec![false; state.tooltips.len()];
    for at in (0..state.tooltips.len()).rev() {
        let tooltip = &state.tooltips[at];
        held[at] = tooltip.hovered_source
            || tooltip.hovered_surface
            || state.tooltips[at + 1..]
                .iter()
                .zip(&held[at + 1..])
                .any(|(child, held)| {
                    *held && child.parent == Some(tooltip.root)
                });
    }

    let mut expired = Vec::new();
    for (tooltip, held) in state.tooltips.iter_mut().zip(held) {
        if tooltip.revealed {
            if held {
                tooltip.hiding = None;
                continue;
            }
            let hiding = tooltip.hiding.get_or_insert(Duration::ZERO);
            *hiding += delta;
            if *hiding >= HIDE_GRACE {
                expired.push(tooltip.root);
            }
            continue;
        }

        if !tooltip.hovered_source {
            continue;
        }
        tooltip.resting += delta;
        if tooltip.resting < DELAY {
            continue;
        }
        let Some(cursor) = cursor else {
            continue;
        };

        // Once shown, this never repositions the tooltip: doing so
        // while a pointer reaches into it to click something would
        // make it a moving target.
        tooltip.revealed = true;
        if let Ok(mut node) = nodes.get_mut(tooltip.root) {
            node.display = Display::Flex;
        }
        for (surface, mark) in &marks {
            if mark.0 == tooltip.root
                && let Ok(mut node) = nodes.get_mut(surface)
            {
                node.left = px(cursor.x + OFFSET.x);
                node.top = px(cursor.y + OFFSET.y);
            }
        }
    }

    for root in expired {
        state.drop_tooltip(root, &mut commands);
    }
}

/// The tooltip itself, on [`MenuSurface`] - the same popup surface a
/// context menu or a dropdown's own list uses - stacked above both
/// per [`crate::theme::Layers::tooltip`], and each nested tooltip
/// above the one it opened from. Pickable, not `Pickable::IGNORE`, so
/// its own `Over`/`Out` can hold it open and content inside it (a
/// link, say) can be clicked.
fn spawn_tooltip<B>(
    world: &mut World,
    root: Entity,
    depth: usize,
    build: B,
) where
    B: Fn(&mut BevyUi) + Send + Sync + 'static,
{
    watch_root::<EditorTheme>(world, root, move |ui: &mut BevyUi| {
        let z = ui.theme.layer.tooltip + depth as i32;

        ui.elem(elem!(
            !MenuSurface,
            z = Some(z),
            inset = UiRect::new(px(0), auto(), px(0), auto()),
        ))
        .insert(TooltipMark(root))
        .observe(
            move |_: On<Pointer<Over>>,
                  mut state: ResMut<TooltipState>| {
                if let Some(tooltip) = state
                    .tooltips
                    .iter_mut()
                    .find(|tooltip| tooltip.root == root)
                {
                    tooltip.hovered_surface = true;
                    tooltip.hiding = None;
                }
            },
        )
        .observe(
            move |_: On<Pointer<Out>>,
                  mut state: ResMut<TooltipState>| {
                if let Some(tooltip) = state
                    .tooltips
                    .iter_mut()
                    .find(|tooltip| tooltip.root == root)
                {
                    tooltip.hovered_surface = false;
                }
            },
        )
        .with(|ui| build(ui));
    });
}
