//! The timeline panel: control bar (play/pause + time readout) and a
//! scrubbable track viewport, edge to edge. No name gutter: a
//! block's own header box already carries its label.

mod block_layout;
mod create;
mod hint;
mod landing;
mod pattern;
mod prune;
mod reorder;
mod retime;
mod time_axis;
mod zoom;

use core::time::Duration;
use std::collections::BTreeSet;

use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, ContextMenuExt as _, FrameProps as _, button,
    column, frame, ghost, icon, label, menu_item, number_field, row,
    tint,
};
use bevy_fynix::{
    AnyView, Bevy, Hovered, Pressed, Prop, ScopedExt as _,
    ViewExt as _, ViewSeq as _, keyed, resource,
};
use bevy_motiongfx::prelude::MotionGfxManager;
use block_layout::Placed;
use moxie_ui::drag::Dragged;
use moxie_ui::elements::{
    Placement, playhead_line, time_label, time_tick, timeline_action,
    timeline_block, timeline_gap, timeline_link,
};
use moxie_ui::field_icon::field_icon;
use moxie_ui::fold::{CHEVRON_OPEN, CHEVRON_SHUT};
use moxie_ui::gaps::{changing, changing_under};
use moxie_ui::icons as ui_icons;
use moxie_ui::theme::{EditorTheme, Spacing};
use pattern::DelayPattern;
use zoom::{FitTimeline, on_track_scroll};

use crate::playback::{
    SeekTo, TogglePlayback, on_seek, on_track_cancel,
    on_track_click_release, on_track_drag, on_track_press,
    on_track_release,
};
use crate::{EditorScene, EditorState, SelectedAction, TimelineView};

/// The timeline's resources and interaction systems.
pub(crate) struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimelineView>()
            .init_resource::<BlockFoldState>()
            .init_resource::<RebuildTick>()
            .add_plugins((
                pattern::plugin,
                retime::plugin,
                reorder::plugin,
                create::plugin,
                zoom::plugin,
            ))
            .add_observer(on_seek);
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
/// A block header's height.
const HEADER_ROW: f32 = 18.0;

/// Viewport where the timeline, track and action UI is displayed.
#[derive(Component, Default, Clone)]
pub(crate) struct TrackViewport;

/// The timeline panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    column((control_bar(), track_area()))
        .width(percent(100.0))
        .height(percent(100.0))
        .gap(0.0)
        .boxed()
}

/// Play/pause, the time readout and the fit button.
fn control_bar() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        let assets = cx.world.resource::<AssetServer>();
        let play = assets.load::<Image>(crate::icons::PLAY);
        let pause = assets.load::<Image>(crate::icons::PAUSE);
        cx.build(
            row((
                button(
                    icon(resource::<EditorState, _>(move |state| {
                        if state.is_playing {
                            pause.clone()
                        } else {
                            play.clone()
                        }
                    }))
                    .size(14.0),
                )
                .padding(UiRect::axes(px(8.0), px(4.0)))
                .on_activate(|world| world.trigger(TogglePlayback)),
                row((
                    number_field::<f32>(
                        changing(shown_secs),
                        |world, secs| world.trigger(SeekTo(secs)),
                    )
                    .width(px(64.0)),
                    label("s"),
                ))
                .align(AlignItems::Center)
                .gap(3.0),
                frame().grow(1.0),
                button(label("Fit"))
                    .width(px(44.0))
                    .height(px(24.0))
                    .on_activate(|world| world.trigger(FitTimeline)),
            ))
            .width(percent(100.0))
            .height(px(CONTROL_BAR_HEIGHT))
            .align(AlignItems::Center)
            .gap(12.0)
            .padding(UiRect::horizontal(px(pad))),
        )
    })
}

/// The playhead's time as the readout shows it.
fn shown_secs(world: &World) -> f32 {
    (current_time(world).as_secs_f32() * 100.0).round() / 100.0
}

/// The scrollable track viewport, filling the whole panel width. The
/// playhead floats over it as a sibling, so the viewport neither
/// scrolls nor clips it.
fn track_area() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let area = cx.build(
            column((
                playhead_line(
                    changing(|world: &World| {
                        px(world
                            .resource::<TimelineView>()
                            .x_from_time(current_time(world)))
                    }),
                    px(TIME_AXIS_HEIGHT),
                ),
                time_axis(),
                clipped_tracks(),
            ))
            .width(percent(100.0))
            .grow(1.0)
            .gap(0.0),
        );
        cx.world
            .entity_mut(area)
            .observe(on_track_press)
            .observe(on_track_drag)
            .observe(on_track_release)
            .observe(on_track_click_release)
            .observe(on_track_cancel)
            .observe(on_track_scroll);
        area
    })
}

/// The time axis ruler above the tracks: a tick every so often and a
/// reading at the major ones, drawn again when its width or the view
/// changes.
fn time_axis() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let axis = cx.build(
            frame()
                .width(percent(100.0))
                .height(px(TIME_AXIS_HEIGHT)),
        );
        let marks = keyed::<EditorTheme, (u32, TimelineView)>(
            changing_under(Some(axis), move |world: &World| {
                axis_view(world, axis)
            }),
            |&(width, view)| axis_marks(width, view),
        )
        .within(frame().width(percent(100.0)).height(percent(100.0)));
        cx.under(axis, |cx| cx.build(marks));
        axis
    })
}

/// The time axis's width and the view it draws, so a change to
/// either redraws the marks. Width is rounded so sub-pixel jitter
/// cannot.
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

/// Every tick and reading across `width` px of `view`.
fn axis_marks(
    width: u32,
    view: TimelineView,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let color = cx.theme().color.text_dim;
        let mut marks = Vec::new();
        for tick in time_axis::ticks(&view, width as f32) {
            let major = tick.label.is_some();
            marks.push(
                time_tick(
                    px(tick.x),
                    px(if major { MAJOR_TICK } else { MINOR_TICK }),
                    color.with_alpha(if major { 0.6 } else { 0.3 }),
                )
                .boxed(),
            );
            if let Some(text) = tick.label {
                marks.push(time_label(px(tick.x), text).boxed());
            }
        }
        cx.build(
            row(marks)
                .width(percent(100.0))
                .height(percent(100.0))
                .gap(0.0),
        )
    })
}

/// Clips the viewport and the hint, both its children, so a
/// scrolled-off hint cannot bleed up over the time axis. The
/// playhead sits outside this on purpose: it runs the ruler's full
/// height.
fn clipped_tracks() -> AnyView<Bevy, EditorTheme> {
    column((viewport(), hint::hint()))
        .width(percent(100.0))
        .grow(1.0)
        .min_height(px(0.0))
        .gap(0.0)
        .overflow(Overflow::clip())
        .boxed()
}

/// The boxes' scroll area. Wheel scrolling is the track's own (see
/// [`on_track_scroll`]), so it has none of its own.
fn viewport() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let space = cx.theme().space;
        cx.build(
            keyed::<EditorTheme, BlockKey>(
                changing(move |world: &World| {
                    block_view(world, space)
                }),
                block_boxes,
            )
            .within(
                column(())
                    .width(percent(100.0))
                    .grow(1.0)
                    .min_width(px(0.0))
                    .min_height(px(0.0))
                    .overflow(Overflow::scroll())
                    .with((TrackViewport, ScrollPosition::default())),
            ),
        )
    })
}

/// `timeline.target_time()`, or zero if no timeline is focused yet.
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

/// The editor scene's animation tree, laid out as nested boxes.
/// Nested boxes are a percent of their parent, so the layout ignores
/// the view and only the root box follows it.
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

/// The boxes plus which one, if any, is selected. The key of the box
/// list: it rebuilds only when a node is added, removed, re-timed,
/// re-nested, reordered, or selection moves onto or off it.
type BlockKey = (Vec<Placed>, Option<Vec<usize>>, u64);

fn block_view(world: &World, space: Spacing) -> BlockKey {
    let selected = world
        .get_resource::<SelectedAction>()
        .and_then(|s| s.0.clone());
    // Two siblings drawn the same size lay out identically whichever
    // order they're in, so a swap leaves the placements elementwise
    // equal and nothing else here would ask for the rebuild that
    // rebinds each box to its new path.
    let tick =
        world.get_resource::<RebuildTick>().map_or(0, |tick| tick.0);
    (block_placements(world, space), selected, tick)
}

/// The boxes, nested the way the tree is: a block's header holds its
/// children, and an action leaf is its own box. Either outlines in
/// the theme's accent when [`SelectedAction`] names its path, and
/// clicking either writes that path in; only the action also lights
/// up under the cursor.
///
/// Under a root of its own, which has no transition, so a rebuild
/// swaps the whole tree at once rather than fading the old one out.
fn block_boxes(key: &BlockKey) -> AnyView<Bevy, EditorTheme> {
    let (placements, selected, _) = key.clone();
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let tree = Tree {
            placements: &placements,
            selected: selected.as_ref(),
            theme: cx.theme(),
            world: cx.world,
            pattern: cx.world.resource::<DelayPattern>().0.clone(),
            chevron: cx
                .world
                .resource::<AssetServer>()
                .load(ui_icons::CHEVRON),
            trash: cx
                .world
                .resource::<AssetServer>()
                .load(ui_icons::TRASH),
        };
        let views = if placements.is_empty() {
            Vec::new()
        } else {
            tree.nodes(0).0
        };
        cx.build(row(views).gap(0.0))
    })
}

/// What building the boxes of one layout reads.
struct Tree<'a> {
    placements: &'a [Placed],
    selected: Option<&'a Vec<usize>>,
    theme: &'a EditorTheme,
    world: &'a World,
    pattern: Handle<Image>,
    chevron: Handle<Image>,
    trash: Handle<Image>,
}

impl Tree<'_> {
    /// The views of `placements[at]` and everything nested under it,
    /// and the index just past that subtree.
    fn nodes(
        &self,
        at: usize,
    ) -> (Vec<AnyView<Bevy, EditorTheme>>, usize) {
        let placed = &self.placements[at];
        let is_selected = self.selected == Some(&placed.path);
        let mut views = Vec::new();
        let mut next = at + 1;

        // Built at zero width even with no delay yet, so a live
        // drag that opens one up has a node already in place to
        // grow.
        if !placed.path.is_empty() {
            views.push(self.gap(placed));
        }
        if let Some([left, top, width, height]) = placed.link_rect() {
            views.push(
                timeline_link(
                    Placement::new(left, top, width, height),
                    self.theme.color.text_dim,
                )
                .tagged(retime::LinkPath(placed.path.clone()))
                .boxed(),
            );
        }

        if placed.label.is_some() {
            let mut inside = vec![self.header(placed)];
            // The root's box has no `delay` of its own to drag -
            // it always starts at zero.
            if !placed.path.is_empty() {
                inside.push(self.edge(placed, retime::Kind::Delay));
            }
            while self.placements.get(next).is_some_and(|child| {
                child.path.len() > placed.path.len()
            }) {
                let (nested, after) = self.nodes(next);
                inside.extend(nested);
                next = after;
            }
            views.push(self.block(placed, is_selected, inside));
        } else {
            views.push(self.action(placed, is_selected));
        }

        (views, next)
    }

    /// The hatched stretch before a node's own delay ends.
    fn gap(&self, placed: &Placed) -> AnyView<Bevy, EditorTheme> {
        timeline_gap(
            Placement::new(
                placed.gap_left(),
                placed.top(),
                placed.gap_width(),
                px(placed.h),
            ),
            self.pattern.clone(),
            self.theme.color.text_dim.with_alpha(0.35),
        )
        .tagged(retime::GapPath(placed.path.clone()))
        .boxed()
    }

    /// A block's box around `inside`. The root's follows the view,
    /// the rest are a percent of their parent.
    fn block(
        &self,
        placed: &Placed,
        is_selected: bool,
        inside: Vec<AnyView<Bevy, EditorTheme>>,
    ) -> AnyView<Bevy, EditorTheme> {
        let (left, width): (Prop<Val>, Prop<Val>) =
            if placed.path.is_empty() {
                let secs = placed.w;
                (
                    resource::<TimelineView, _>(|view| {
                        px(view.x_from_time(Duration::ZERO))
                    })
                    .into(),
                    resource::<TimelineView, _>(move |view| {
                        px(secs * view.px_per_second)
                    })
                    .into(),
                )
            } else {
                (placed.left().into(), placed.width().into())
            };
        timeline_block(
            Placement::new(left, placed.top(), width, px(placed.h)),
            is_selected,
            inside,
        )
        .tagged(retime::BoxPath(placed.path.clone()))
        .boxed()
    }

    /// A block's header: its name (or combinator, if unnamed) beside
    /// its fold chevron, clickable to select; the chevron alone
    /// toggles the fold.
    fn header(&self, placed: &Placed) -> AnyView<Bevy, EditorTheme> {
        let path = placed.path.clone();
        let rotation = if placed.folded {
            CHEVRON_SHUT
        } else {
            CHEVRON_OPEN
        };
        let fold_path = path.clone();
        let chevron = button(
            icon(self.chevron.clone())
                .size(7.0)
                .rotation(rotation)
                .when::<Dragged, _>(|icon, _: &EditorTheme| {
                icon.opacity(0.2)
            }),
        )
        .padding(UiRect::all(px(3.0)))
        .rules(tint)
        .toned(Tone::Faint)
        .on_activate(move |world| toggle_folded(world, &fold_path));
        let name = label(placed.label.clone().unwrap_or_default())
            .wrap(false)
            .opacity(0.8)
            .when::<Dragged, _>(|label, _: &EditorTheme| {
                label.opacity(0.2)
            });

        let select_path = path.clone();
        let header = button(
            row((chevron, name)).align(AlignItems::Center).gap(4.0),
        )
        .width(percent(100.0))
        .height(px(HEADER_ROW))
        .justify(JustifyContent::FlexStart)
        .padding(UiRect::axes(px(4.0), px(2.0)))
        .radius(0.0)
        .rules(ghost)
        .on_activate(move |world| select(world, &select_path));
        let header = reorder::body(header, path.clone());
        // The root block has nothing above it to delete it from.
        if path.is_empty() {
            header
        } else {
            self.deletable(header, path)
        }
    }

    /// An action leaf's own box, with its two edge handles.
    fn action(
        &self,
        placed: &Placed,
        is_selected: bool,
    ) -> AnyView<Bevy, EditorTheme> {
        let image = placed.field.as_ref().and_then(|field| {
            let registry =
                self.world.resource::<AppTypeRegistry>().read();
            field_icon(
                &registry,
                &field.type_name().to_string(),
                field.path(),
            )
        });
        let image = image.map(|path| {
            self.world.resource::<AssetServer>().load::<Image>(path)
        });
        let name = placed.name.clone().unwrap_or_else(|| {
            if placed.draft {
                "Draft".to_string()
            } else {
                String::new()
            }
        });
        let path = placed.path.clone();
        let select_path = path.clone();
        let action = timeline_action(
            Placement::new(
                placed.left(),
                placed.top(),
                placed.width(),
                px(placed.h),
            ),
            name,
            image,
            placed.draft,
            is_selected,
        )
        .tagged(retime::BoxPath(path.clone()))
        .on_activate(move |world| select(world, &select_path));
        let action = reorder::body(action, path.clone());
        let action = self.deletable(action, path.clone());

        let edges = vec![
            self.edge(placed, retime::Kind::Delay),
            self.edge(placed, retime::Kind::Resize),
        ];
        AnyView::<Bevy, EditorTheme>::new(move |cx| {
            let node = cx.build(action);
            cx.under(node, |cx| edges.build_each(cx));
            node
        })
    }

    /// A thin strip on one edge of the box it is built inside, wired
    /// to `kind` via [`retime::edge`].
    fn edge(
        &self,
        placed: &Placed,
        kind: retime::Kind,
    ) -> AnyView<Bevy, EditorTheme> {
        let accent = self.theme.color.accent;
        let inset = match kind {
            retime::Kind::Delay => {
                UiRect::new(Val::ZERO, auto(), Val::ZERO, auto())
            }
            retime::Kind::Resize => {
                UiRect::new(auto(), Val::ZERO, Val::ZERO, auto())
            }
        };
        let handle = frame()
            .position(PositionType::Absolute)
            .inset(inset)
            .width(px(retime::EDGE_HANDLE_PX))
            .height(percent(100.0))
            .when::<Hovered, _>(move |frame, _: &EditorTheme| {
                frame.fill(accent.with_alpha(0.35))
            })
            .when::<Pressed, _>(move |frame, _: &EditorTheme| {
                frame.fill(accent.with_alpha(0.6))
            });
        retime::edge(handle, placed.path.clone(), kind)
    }

    /// `view` with a right-click menu deleting the node at `path`.
    fn deletable(
        &self,
        view: AnyView<Bevy, EditorTheme>,
        path: Vec<usize>,
    ) -> AnyView<Bevy, EditorTheme> {
        let trash = self.trash.clone();
        let gap = self.theme.space.md;
        view.context_menu(move || {
            let path = path.clone();
            (menu_item(
                row((
                    icon(trash.clone()),
                    label("Delete").wrap(false),
                ))
                .align(AlignItems::Center)
                .gap(gap)
                .toned(Tone::Critical),
            )
            .on_activate(move |world| {
                reorder::delete(world, &path);
            }),)
        })
        .boxed()
    }
}

/// Selects the node at `path`.
fn select(world: &mut World, path: &[usize]) {
    world.resource_mut::<SelectedAction>().0 = Some(path.to_vec());
}

#[cfg(test)]
mod tests {
    use bevy::ui::widget::ImageNode;
    use bevy_motiongfx::scene::backend::Backend;
    use motiongfx_scene::block::Node as SceneNode;
    use moxie_ui::elements::Selected;

    use super::retime::BoxPath;
    use super::*;
    use crate::tests::harness::{Editor, SETTLE};

    fn draft(name: &str) -> SceneNode<Backend> {
        SceneNode::Draft {
            delay: None,
            duration: Duration::from_secs(1),
            name: Some(name.to_string()),
        }
    }

    /// An editor whose timeline holds a draft per name.
    fn editor_with(names: &[&str]) -> Editor {
        let mut editor = Editor::new();
        let world = editor.world();
        let mut scene = world.resource_mut::<EditorScene>();
        for name in names {
            scene.edit().animation.children.push(draft(name));
        }
        editor.step(SETTLE);
        editor
    }

    fn boxes(editor: &mut Editor) -> Vec<Vec<usize>> {
        let world = editor.world();
        let mut paths = world
            .query::<&BoxPath>()
            .iter(world)
            .map(|path| path.0.clone())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    #[test]
    fn the_panel_has_its_controls() {
        let mut editor = Editor::new();
        editor.text("Fit");
        editor.text("s");
        let world = editor.world();
        assert_eq!(
            world.query::<&TrackViewport>().iter(world).count(),
            1
        );
    }

    #[test]
    fn an_empty_timeline_draws_no_boxes() {
        let mut editor = Editor::new();
        assert!(boxes(&mut editor).is_empty());
    }

    #[test]
    fn each_node_gets_a_box_under_the_root() {
        let mut editor = editor_with(&["Intro", "Outro"]);
        assert_eq!(
            boxes(&mut editor),
            vec![vec![], vec![0], vec![1]]
        );
        editor.text("Intro");
        editor.text("Outro");
    }

    #[test]
    fn pressing_a_box_selects_it() {
        let mut editor = editor_with(&["Intro", "Outro"]);
        editor.press("Outro");

        assert_eq!(
            editor.world().resource::<SelectedAction>().0,
            Some(vec![1])
        );
        let world = editor.world();
        let selected = world
            .query_filtered::<&BoxPath, With<Selected>>()
            .iter(world)
            .map(|path| path.0.clone())
            .collect::<Vec<_>>();
        assert_eq!(selected, vec![vec![1]]);
    }

    #[test]
    fn deleting_a_node_removes_its_box() {
        let mut editor = editor_with(&["Intro", "Outro"]);
        reorder::delete(editor.world(), &[0]);
        editor.step(SETTLE);

        assert_eq!(boxes(&mut editor), vec![vec![], vec![0]]);
        assert!(editor.texts("Intro").is_empty());
        editor.text("Outro");
    }

    #[test]
    fn a_reorder_rebuilds_the_boxes() {
        let mut editor = editor_with(&["Intro", "Outro"]);
        {
            let world = editor.world();
            world
                .resource_mut::<EditorScene>()
                .edit()
                .animation
                .children
                .swap(0, 1);
            RebuildTick::bump_in(world);
        }
        editor.step(SETTLE);

        // The first box now carries the other name.
        let world = editor.world();
        let first = world
            .query::<(&BoxPath, &Children)>()
            .iter(world)
            .find(|(path, _)| path.0 == [0])
            .map(|(_, children)| children.len())
            .unwrap_or(0);
        assert!(first > 0);
        editor.text("Outro");
    }

    #[test]
    fn folding_a_block_hides_what_it_holds() {
        let mut editor = editor_with(&["Intro"]);
        toggle_folded(editor.world(), &[]);
        editor.step(SETTLE);

        assert_eq!(boxes(&mut editor), vec![Vec::<usize>::new()]);
        toggle_folded(editor.world(), &[]);
        editor.step(SETTLE);
        assert_eq!(boxes(&mut editor), vec![vec![], vec![0]]);
    }

    #[test]
    fn the_play_button_follows_the_playing_state() {
        let mut editor = Editor::new();
        let icon_path = |editor: &mut Editor| {
            let world = editor.world();
            let play = crate::icons::PLAY;
            let pause = crate::icons::PAUSE;
            world
                .query::<&ImageNode>()
                .iter(world)
                .filter_map(|image| {
                    image.image.path().map(ToString::to_string)
                })
                .find(|path| path == play || path == pause)
        };
        assert_eq!(
            icon_path(&mut editor).as_deref(),
            Some(crate::icons::PLAY)
        );

        editor.world().spawn(
            bevy_motiongfx::prelude::RealtimePlayer {
                is_playing: true,
                time_scale: 1.0,
            },
        );
        editor.step(SETTLE);
        assert_eq!(
            icon_path(&mut editor).as_deref(),
            Some(crate::icons::PAUSE)
        );
    }

    #[test]
    fn the_axis_marks_are_drawn_again_when_the_zoom_changes() {
        let mut editor = Editor::new();
        let marks = |editor: &mut Editor| {
            let world = editor.world();
            world
                .query::<&bevy::ui::widget::Text>()
                .iter(world)
                .count()
        };
        let before = marks(&mut editor);
        editor.world().resource_mut::<TimelineView>().zoom_to(
            0.0,
            Duration::ZERO,
            20.0,
        );
        editor.step(SETTLE);
        assert_ne!(marks(&mut editor), before);
    }

    #[test]
    fn toggling_a_fold_flips_it() {
        let mut world = World::new();
        world.init_resource::<BlockFoldState>();
        toggle_folded(&mut world, &[1, 2]);
        assert!(
            world
                .resource::<BlockFoldState>()
                .paths()
                .contains(&vec![1, 2])
        );
        toggle_folded(&mut world, &[1, 2]);
        assert!(
            world.resource::<BlockFoldState>().paths().is_empty()
        );
    }
}
