//! A hover tag: UI that appears near the cursor after a short pause.
//!
//! Attached per-entity with [`tooltip`] or [`tooltip_with`], the same
//! way `draggable_field` wires up a drag: the hover lives in one shared
//! `TooltipState` rather than on the element itself.
//!
//! The tag is itself interactable, not just decoration: it stays up
//! while the pointer sits on it, not only on the source, so a link or
//! a button inside one can be reached and clicked. A source inside a
//! tag opens a tag of its own on top, and each tag stays up while the
//! pointer is on its source, on it, or on a tag opened from it.

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

/// How long the pointer has to rest on the source before the tag
/// appears.
const DELAY: Duration = Duration::from_millis(500);

/// How long neither the source nor the tag may be hovered before it
/// hides - long enough to cross the gap between them to reach the
/// tag itself.
const HIDE_GRACE: Duration = Duration::from_millis(150);

/// Where the tag sits relative to the cursor it appeared at, clear of
/// the pointer hotspot.
const OFFSET: Vec2 = Vec2::new(12.0, 18.0);

/// Every tag on its way to showing or showing, oldest first. A tag is
/// always pushed after the tag its source sits in.
#[derive(Resource, Default)]
struct TooltipState {
    tags: Vec<Tag>,
}

/// One tag, from the moment its source is hovered.
struct Tag {
    /// The tag's root. Built hidden as soon as the source is hovered,
    /// and shown once the pointer has rested [`DELAY`].
    root: Entity,
    source: Entity,
    /// The root of the tag `source` sits in, if any. At most one tag
    /// per parent exists at a time.
    parent: Option<Entity>,
    /// How many tags it is stacked on.
    depth: usize,
    revealed: bool,
    /// How long the pointer has rested on `source`, before it shows.
    resting: Duration,
    hovered_source: bool,
    hovered_tag: bool,
    /// Counts up once nothing holding the tag open is hovered; `None`
    /// while something is.
    hiding: Option<Duration>,
}

impl TooltipState {
    /// Drops the tag at `root`, and every tag opened from it.
    fn drop_tag(&mut self, root: Entity, commands: &mut Commands) {
        let mut doomed = vec![root];
        let mut at = 0;
        while let Some(&parent) = doomed.get(at) {
            doomed.extend(
                self.tags
                    .iter()
                    .filter(|tag| tag.parent == Some(parent))
                    .map(|tag| tag.root),
            );
            at += 1;
        }

        self.tags.retain(|tag| {
            let keep = !doomed.contains(&tag.root);
            if !keep {
                commands.entity(tag.root).despawn();
            }
            keep
        });
    }
}

/// A tag's own surface, so the tag it belongs to can be found from
/// anything inside it.
#[derive(Component)]
struct TooltipMark(Entity);

/// Registers [`tick`], the system [`tooltip_with`] otherwise has
/// nothing to drive it.
pub(crate) struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TooltipState>()
            .add_systems(Update, tick);
    }
}

/// Makes `elem` show `text` near the cursor after a short hover, and
/// keeps it up as long as the pointer stays on `elem` or the tag.
pub fn tooltip(
    elem: &mut impl WorldEntityMut,
    text: impl Into<String>,
) {
    let text = text.into();
    tooltip_with(elem, move |ui| {
        let h = ui.theme.space.md;
        let v = ui.theme.space.xs;
        let size = ui.theme.text.small;
        let color = ui.theme.color.text;
        let text = text.clone();

        // Its own room beyond `MenuSurface`'s own padding: that's
        // sized for a menu row, snug on a bare tag.
        ui.elem(elem!(Frame, padding = UiRect::axes(px(h), px(v))))
            .with(move |ui| {
                ui.elem(elem!(
                    Label,
                    text = text,
                    size = size,
                    color = color,
                    wrap = false
                ));
            });
    });
}

/// Makes `elem` show whatever `build` makes inside a [`MenuSurface`]
/// near the cursor after a short hover, and keeps it up as long as
/// the pointer stays on `elem`, the tag, or a tag opened from it.
///
/// `build` runs each time the pointer enters `elem`, so it should be
/// cheap.
pub fn tooltip_with<B>(elem: &mut impl WorldEntityMut, build: B)
where
    B: Fn(&mut BevyUi) + Clone + Send + Sync + 'static,
{
    let source = elem.id();
    elem.observe(
        move |_: On<Pointer<Over>>,
              mut state: ResMut<TooltipState>,
              parents: Query<&ChildOf>,
              marks: Query<&TooltipMark>,
              mut commands: Commands| {
            if let Some(tag) =
                state.tags.iter_mut().find(|tag| tag.source == source)
            {
                tag.hovered_source = true;
                tag.hiding = None;
                return;
            }

            let parent = enclosing_tag(source, &parents, &marks);

            // The pointer moved to another source at this level while
            // its neighbour's tag was still up: that tag goes.
            let neighbour = state
                .tags
                .iter()
                .find(|tag| tag.parent == parent)
                .map(|tag| tag.root);
            if let Some(neighbour) = neighbour {
                state.drop_tag(neighbour, &mut commands);
            }

            let depth = parent
                .and_then(|parent| {
                    state.tags.iter().find(|tag| tag.root == parent)
                })
                .map_or(0, |tag| tag.depth + 1);

            // `Pickable::IGNORE`, or this full-screen node blocks the
            // very hover that showed it: it swallows the pointer, the
            // source loses `Over`, the tag despawns, hover resumes,
            // and it loops.
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
            state.tags.push(Tag {
                root,
                source,
                parent,
                depth,
                revealed: false,
                resting: Duration::ZERO,
                hovered_source: true,
                hovered_tag: false,
                hiding: None,
            });

            let build = build.clone();
            commands.queue(move |world: &mut World| {
                spawn_tag(world, root, depth, build)
            });
        },
    )
    .observe(
        move |_: On<Pointer<Out>>,
              mut state: ResMut<TooltipState>,
              mut commands: Commands| {
            let Some(tag) = state
                .tags
                .iter_mut()
                .find(|tag| tag.source == source)
            else {
                return;
            };
            tag.hovered_source = false;

            // Never shown, so nothing to hold open.
            if !tag.revealed {
                let root = tag.root;
                state.drop_tag(root, &mut commands);
            }
        },
    );
}

/// The tag `entity` sits inside, if any.
fn enclosing_tag(
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

/// Counts each tag's show delay, shows it once it elapses, and counts
/// the hide grace once nothing holding a shown tag open is hovered any
/// longer.
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

    // A tag is held open by its source, by itself, or by a tag opened
    // from it. Deepest first: a tag opened from another comes after
    // it.
    let mut held = vec![false; state.tags.len()];
    for at in (0..state.tags.len()).rev() {
        let tag = &state.tags[at];
        held[at] = tag.hovered_source
            || tag.hovered_tag
            || state.tags[at + 1..].iter().zip(&held[at + 1..]).any(
                |(child, held)| {
                    *held && child.parent == Some(tag.root)
                },
            );
    }

    let mut expired = Vec::new();
    for (tag, held) in state.tags.iter_mut().zip(held) {
        if tag.revealed {
            if held {
                tag.hiding = None;
                continue;
            }
            let hiding = tag.hiding.get_or_insert(Duration::ZERO);
            *hiding += delta;
            if *hiding >= HIDE_GRACE {
                expired.push(tag.root);
            }
            continue;
        }

        if !tag.hovered_source {
            continue;
        }
        tag.resting += delta;
        if tag.resting < DELAY {
            continue;
        }
        let Some(cursor) = cursor else {
            continue;
        };

        // Once shown, this never repositions the tag: doing so while
        // a pointer reaches into it to click something would make it
        // a moving target.
        tag.revealed = true;
        if let Ok(mut node) = nodes.get_mut(tag.root) {
            node.display = Display::Flex;
        }
        for (surface, mark) in &marks {
            if mark.0 == tag.root
                && let Ok(mut node) = nodes.get_mut(surface)
            {
                node.left = px(cursor.x + OFFSET.x);
                node.top = px(cursor.y + OFFSET.y);
            }
        }
    }

    for root in expired {
        state.drop_tag(root, &mut commands);
    }
}

/// The tag itself, on [`MenuSurface`] - the same popup surface a
/// context menu or a dropdown's own list uses - stacked above both
/// per [`crate::theme::Layers::tooltip`], and each nested tag above
/// the one it opened from. Pickable, not `Pickable::IGNORE`, so its
/// own `Over`/`Out` can hold it open and content inside it (a link,
/// say) can be clicked.
fn spawn_tag<B>(
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
                if let Some(tag) =
                    state.tags.iter_mut().find(|tag| tag.root == root)
                {
                    tag.hovered_tag = true;
                    tag.hiding = None;
                }
            },
        )
        .observe(
            move |_: On<Pointer<Out>>,
                  mut state: ResMut<TooltipState>| {
                if let Some(tag) =
                    state.tags.iter_mut().find(|tag| tag.root == root)
                {
                    tag.hovered_tag = false;
                }
            },
        )
        .with(|ui| build(ui));
    });
}
