//! The core against a fake backend: a flat list of nodes holding some
//! text at a size, and one number the text can be bound to.

use core::time::Duration;

use motiongfx_interp::ease;
use motiongfx_interp::interpolation::Interpolation;

use crate::{
    AnyView, Backend, Curve, Cx, Frame, Leaf, Motion, MotionTokens,
    Mounted, Prop, Styled, Tween, View, derived,
};

#[derive(Default)]
struct World {
    nodes: Vec<Option<Node>>,
    count: i32,
}

#[derive(Default)]
struct Node {
    parent: Option<usize>,
    text: String,
    size: f32,
}

struct Fake;

impl Backend for Fake {
    type World = World;
    type Node = usize;

    fn spawn(world: &mut World, parent: Option<usize>) -> usize {
        world.nodes.push(Some(Node {
            parent,
            ..Node::default()
        }));
        world.nodes.len() - 1
    }

    fn exists(world: &World, node: usize) -> bool {
        world.nodes.get(node).is_some_and(Option::is_some)
    }
}

impl World {
    fn node(&self, node: usize) -> &Node {
        self.nodes[node].as_ref().expect("a live node")
    }

    fn children(&self, parent: usize) -> Vec<usize> {
        (0..self.nodes.len())
            .filter(|&node| {
                self.nodes[node]
                    .as_ref()
                    .is_some_and(|n| n.parent == Some(parent))
            })
            .collect()
    }
}

/// What the fake backend's one leaf reads from a theme.
trait Sizes {
    fn body(&self) -> f32;
}

/// A run of text.
struct Text {
    text: Prop<World, String>,
    size: Prop<World, f32>,
    motion: Option<Motion>,
}

fn text(text: impl Into<Prop<World, String>>) -> Text {
    Text {
        text: text.into(),
        ..Text::unset()
    }
}

impl Text {
    fn size(mut self, size: impl Into<Prop<World, f32>>) -> Self {
        self.size = size.into();
        self
    }

    fn transition(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }
}

impl Styled for Text {
    fn unset() -> Self {
        Self {
            text: Prop::Unset,
            size: Prop::Unset,
            motion: None,
        }
    }

    fn over(self, below: Self) -> Self {
        Self {
            text: self.text.or(below.text),
            size: self.size.or(below.size),
            motion: self.motion.or(below.motion),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Shown {
    text: String,
    size: f32,
}

impl Shown {
    fn interp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            text: to.text.clone(),
            size: <f32 as Interpolation<()>>::interp(
                &from.size, &to.size, t,
            ),
        }
    }
}

impl<T: Sizes + MotionTokens> Leaf<Fake, T> for Text {
    type Snapshot = Shown;

    fn prepare(_: &mut World, _: usize) {}

    fn snapshot(&self, world: &World, theme: &T) -> Shown {
        Shown {
            text: self.text.get(world).unwrap_or_default(),
            size: self.size.get(world).unwrap_or(theme.body()),
        }
    }

    fn write(shown: &Shown, world: &mut World, node: usize) {
        let node = world.nodes[node].as_mut().expect("a live node");
        node.text = shown.text.clone();
        node.size = shown.size;
    }

    fn is_live(&self) -> bool {
        self.text.is_bound() || self.size.is_bound()
    }

    fn tween(&self, theme: &T) -> Option<Tween<Shown>> {
        let curve = theme.motion(self.motion?);
        Some(Tween {
            curve,
            interp: Shown::interp,
        })
    }
}

/// One app's theme.
struct Warm;

impl Sizes for Warm {
    fn body(&self) -> f32 {
        14.0
    }
}

impl MotionTokens for Warm {
    fn motion(&self, _: Motion) -> Curve {
        Curve {
            duration: Duration::from_secs(1),
            ease: ease::linear,
        }
    }
}

/// Another app's, stored nothing like the first.
struct Cold {
    sizes: [f32; 1],
}

impl Sizes for Cold {
    fn body(&self) -> f32 {
        self.sizes[0]
    }
}

impl MotionTokens for Cold {
    fn motion(&self, _: Motion) -> Curve {
        Curve {
            duration: Duration::ZERO,
            ease: ease::linear,
        }
    }
}

/// A world, a theme and its mounted leaves, built into together.
struct Ui<T> {
    world: World,
    theme: T,
    mounted: Mounted<Fake, T>,
}

impl<T: 'static> Ui<T> {
    fn new(theme: T) -> Self {
        Self {
            world: World::default(),
            theme,
            mounted: Mounted::default(),
        }
    }

    fn build(&mut self, view: impl View<Fake, T>) -> usize {
        let mut cx =
            Cx::new(&mut self.world, &self.theme, &mut self.mounted);
        view.build(&mut cx)
    }

    fn update(&mut self, delta: Duration, reduced_motion: bool) {
        self.mounted.update(
            &mut self.world,
            &self.theme,
            Frame {
                delta,
                reduced_motion,
            },
        );
    }

    /// A root node with whatever `build` puts under it.
    fn under(
        &mut self,
        build: impl FnOnce(&mut Cx<'_, Fake, T>) + 'static,
    ) -> Vec<usize> {
        let root =
            self.build(AnyView::new(|cx: &mut Cx<'_, Fake, T>| {
                let root = cx.spawn();
                cx.under(root, build);
                root
            }));
        self.world.children(root)
    }
}

#[test]
fn an_unset_prop_falls_back_to_the_theme() {
    let mut ui = Ui::new(Warm);
    let node = ui.build(text("Save"));

    assert_eq!(ui.world.node(node).text, "Save");
    assert_eq!(ui.world.node(node).size, 14.0);
}

#[test]
fn the_same_view_works_under_two_unrelated_themes() {
    let mut warm = Ui::new(Warm);
    let mut cold = Ui::new(Cold { sizes: [20.0] });
    let in_warm = warm.build(text("x"));
    let in_cold = cold.build(text("x"));

    assert_eq!(warm.world.node(in_warm).size, 14.0);
    assert_eq!(cold.world.node(in_cold).size, 20.0);
}

#[test]
fn a_set_rule_fills_only_what_the_call_site_left_unset() {
    let mut ui = Ui::new(Warm);
    let [ruled, explicit] = ui.under(|cx| {
        cx.set::<Text>(|t, _| t.size(20.0));
        cx.build(text("ruled"));
        cx.build(text("explicit").size(9.0));
    })[..] else {
        panic!("two nodes");
    };

    assert_eq!(ui.world.node(ruled).size, 20.0);
    assert_eq!(ui.world.node(explicit).size, 9.0, "call site wins");
}

#[test]
fn a_rule_can_read_the_theme() {
    let mut ui = Ui::new(Warm);
    let nodes = ui.under(|cx| {
        cx.set::<Text>(|t, theme: &Warm| t.size(theme.body() * 2.0));
        cx.build(text("big"));
    });

    assert_eq!(ui.world.node(nodes[0]).size, 28.0);
}

#[test]
fn an_inner_scope_wins_and_ends_with_its_scope() {
    let mut ui = Ui::new(Warm);
    let [inner, after] = ui.under(|cx| {
        cx.set::<Text>(|t, _| t.size(20.0));
        cx.scope(|cx| {
            cx.set::<Text>(|t, _| t.size(30.0));
            cx.build(text("inner"));
        });
        cx.build(text("after"));
    })[..] else {
        panic!("two nodes");
    };

    assert_eq!(ui.world.node(inner).size, 30.0);
    assert_eq!(ui.world.node(after).size, 20.0);
}

#[test]
fn a_show_rule_wins_over_the_call_site() {
    let mut ui = Ui::new(Warm);
    let nodes = ui.under(|cx| {
        cx.show::<Text>(|t, _| t.size(1.0));
        cx.build(text("x").size(9.0));
    });

    assert_eq!(ui.world.node(nodes[0]).size, 1.0);
}

#[test]
fn a_bound_prop_follows_the_world() {
    let mut ui = Ui::new(Warm);
    ui.world.count = 1;
    let bound = ui.build(text(derived(|world: &World| {
        world.count.to_string()
    })));
    let fixed = ui.build(text("fixed"));
    assert_eq!(ui.world.node(bound).text, "1");

    ui.world.count = 2;
    ui.update(Duration::ZERO, false);

    assert_eq!(ui.world.node(bound).text, "2");
    assert_eq!(ui.world.node(fixed).text, "fixed");
    assert_eq!(
        ui.mounted.len(),
        1,
        "only what can change stays mounted"
    );
}

#[test]
fn a_gone_node_is_dropped() {
    let mut ui = Ui::new(Warm);
    let node = ui.build(text(derived(|world: &World| {
        world.count.to_string()
    })));
    ui.world.nodes[node] = None;
    ui.update(Duration::ZERO, false);

    assert!(ui.mounted.is_empty());
}

/// A text whose size follows `world.count`, travelling when asked.
fn counted(motion: Option<Motion>) -> Text {
    let text =
        text("x").size(derived(|world: &World| world.count as f32));
    match motion {
        Some(motion) => text.transition(motion),
        None => text,
    }
}

#[test]
fn nothing_travels_unless_asked() {
    let mut ui = Ui::new(Warm);
    ui.world.count = 10;
    let node = ui.build(counted(None));

    ui.world.count = 20;
    ui.update(Duration::from_millis(100), false);

    assert_eq!(ui.world.node(node).size, 20.0);
}

#[test]
fn a_transition_reaches_its_target_over_the_curve() {
    let mut ui = Ui::new(Warm);
    ui.world.count = 10;
    let node = ui.build(counted(Some(Motion::Interact)));

    ui.world.count = 20;
    ui.update(Duration::ZERO, false);
    ui.update(Duration::from_millis(500), false);
    assert_eq!(ui.world.node(node).size, 15.0);

    ui.update(Duration::from_millis(500), false);
    assert_eq!(ui.world.node(node).size, 20.0);
}

#[test]
fn an_interrupted_transition_starts_from_where_it_got_to() {
    let mut ui = Ui::new(Warm);
    ui.world.count = 0;
    let node = ui.build(counted(Some(Motion::Interact)));

    ui.world.count = 100;
    ui.update(Duration::ZERO, false);
    ui.update(Duration::from_millis(500), false);
    assert_eq!(ui.world.node(node).size, 50.0);

    // Heads back to 0 from 50, not from 100.
    ui.world.count = 0;
    ui.update(Duration::ZERO, false);
    ui.update(Duration::from_millis(500), false);
    assert_eq!(ui.world.node(node).size, 25.0);
}

#[test]
fn reduced_motion_snaps() {
    let mut ui = Ui::new(Warm);
    ui.world.count = 10;
    let node = ui.build(counted(Some(Motion::Interact)));

    ui.world.count = 20;
    ui.update(Duration::from_millis(100), true);

    assert_eq!(ui.world.node(node).size, 20.0);
}

#[test]
fn a_zero_length_curve_snaps() {
    let mut ui = Ui::new(Cold { sizes: [12.0] });
    ui.world.count = 10;
    let node = ui.build(counted(Some(Motion::Interact)));

    ui.world.count = 20;
    ui.update(Duration::from_millis(1), false);

    assert_eq!(ui.world.node(node).size, 20.0);
}
