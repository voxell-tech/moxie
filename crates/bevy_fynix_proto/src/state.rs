//! Interaction states as components on a node, and the rules and
//! transitions a leaf attaches to them.

use bevy::prelude::*;

use crate::cx::Cx;
use crate::tokens::{Curve, Motion, MotionTokens};
use crate::transition::{Interpolate, Tween};
use crate::view::{Leaf, Styled};

/// The pointer is over the node.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Hovered;

/// A pointer button is down on the node.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Pressed;

fn over(over: On<Pointer<Over>>, mut commands: Commands) {
    commands.entity(over.event_target()).insert(Hovered);
}

fn out(out: On<Pointer<Out>>, mut commands: Commands) {
    commands
        .entity(out.event_target())
        .remove::<(Hovered, Pressed)>();
}

fn press(press: On<Pointer<Press>>, mut commands: Commands) {
    commands.entity(press.event_target()).insert(Pressed);
}

fn release(release: On<Pointer<Release>>, mut commands: Commands) {
    commands.entity(release.event_target()).remove::<Pressed>();
}

/// An edit of a snapshot, with the theme in hand.
type Edit<S, T> = Box<dyn Fn(&mut S, &T) + Send + Sync>;

/// An edit that holds while a state component is on the node.
struct StateRule<S, T> {
    holds: fn(&World, Entity) -> bool,
    apply: Edit<S, T>,
}

/// A transition to attach, waiting for the theme to name its curve.
struct Move<S, T> {
    motion: Motion,
    curve: fn(&T, Motion) -> Curve,
    lerp: fn(&S, &S, f32) -> S,
}

impl<S, T> Clone for Move<S, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S, T> Copy for Move<S, T> {}

/// A leaf with state rules and a transition attached.
///
/// It is a [`Leaf`] itself, so it mounts like any other, and it is
/// [`Styled`], so set rules for `Stateful<L, T>` can add rules to
/// every such leaf in a scope.
pub struct Stateful<L: Leaf<T>, T> {
    leaf: L,
    rules: Vec<StateRule<L::Snapshot, T>>,
    motion: Option<Move<L::Snapshot, T>>,
}

impl<L: Leaf<T>, T: Send + Sync + 'static> Stateful<L, T> {
    fn new(leaf: L) -> Self {
        Self {
            leaf,
            rules: Vec::new(),
            motion: None,
        }
    }

    /// While the component `S` is on the node, `rule` edits the
    /// resolved values. Rules apply in the order added, after the
    /// call-site values and any bound props.
    pub fn when<S: Component>(
        mut self,
        rule: impl Fn(&mut L::Snapshot, &T) + Send + Sync + 'static,
    ) -> Self {
        self.rules.push(StateRule {
            holds: |world, node| world.entity(node).contains::<S>(),
            apply: Box::new(rule),
        });
        self
    }

    /// Makes every change to the written values travel over the
    /// theme's curve for `motion`.
    pub fn transition(mut self, motion: Motion) -> Self
    where
        T: MotionTokens,
        L::Snapshot: Interpolate,
    {
        self.motion = Some(Move {
            motion,
            curve: T::motion,
            lerp: <L::Snapshot as Interpolate>::lerp,
        });
        self
    }
}

/// What any leaf can be given state rules and a transition with.
pub trait StateExt<T: Send + Sync + 'static>: Leaf<T> {
    /// This leaf with a rule for the state `S`.
    fn when<S: Component>(
        self,
        rule: impl Fn(&mut Self::Snapshot, &T) + Send + Sync + 'static,
    ) -> Stateful<Self, T> {
        Stateful::new(self).when::<S>(rule)
    }

    /// This leaf with a transition.
    fn transition(self, motion: Motion) -> Stateful<Self, T>
    where
        T: MotionTokens,
        Self::Snapshot: Interpolate,
    {
        Stateful::new(self).transition(motion)
    }
}

impl<T: Send + Sync + 'static, L: Leaf<T>> StateExt<T> for L {}

impl<T: Send + Sync + 'static, L: Leaf<T>> Styled for Stateful<L, T> {
    fn unset() -> Self {
        Self::new(L::unset())
    }

    fn over(mut self, below: Self) -> Self {
        let mut rules = below.rules;
        rules.append(&mut self.rules);
        Self {
            leaf: self.leaf.over(below.leaf),
            rules,
            motion: self.motion.or(below.motion),
        }
    }
}

impl<T: Send + Sync + 'static, L: Leaf<T>> Leaf<T>
    for Stateful<L, T>
{
    type Snapshot = L::Snapshot;

    fn prepare(world: &mut World, node: Entity) {
        L::prepare(world, node);
        world
            .entity_mut(node)
            .observe(over)
            .observe(out)
            .observe(press)
            .observe(release);
    }

    fn snapshot(&self, world: &World, theme: &T) -> L::Snapshot {
        self.leaf.snapshot(world, theme)
    }

    fn write(
        snapshot: &L::Snapshot,
        world: &mut World,
        node: Entity,
    ) {
        L::write(snapshot, world, node);
    }

    fn is_live(&self) -> bool {
        self.leaf.is_live() || !self.rules.is_empty()
    }

    fn resolve(self, cx: &Cx<'_, crate::Bevy, T>) -> Self {
        let Self {
            leaf,
            rules,
            motion,
        } = self;
        // Rules for the bare leaf, then rules for the wrapped one.
        let leaf = L::resolve(leaf, cx);
        cx.resolve(Self {
            leaf,
            rules,
            motion,
        })
    }

    fn adjust(
        &self,
        snapshot: &mut L::Snapshot,
        world: &World,
        node: Entity,
        theme: &T,
    ) {
        self.leaf.adjust(snapshot, world, node, theme);
        for rule in &self.rules {
            if (rule.holds)(world, node) {
                (rule.apply)(snapshot, theme);
            }
        }
    }

    fn tween(&self, theme: &T) -> Option<Tween<L::Snapshot>> {
        match self.motion {
            Some(motion) => Some(Tween {
                curve: (motion.curve)(theme, motion.motion),
                lerp: motion.lerp,
            }),
            None => self.leaf.tween(theme),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::mounted::Mounted;
    use crate::tokens::{TextTokens, Tone};
    use crate::transition::ReducedMotion;
    use crate::views::{Label, LabelSnapshot, label};
    use crate::{
        AnyView, Bevy, FynixProtoPlugin, Theme, derived, mount,
    };

    struct Test;

    impl TextTokens for Test {
        fn tone(&self, tone: Tone) -> Color {
            match tone {
                Tone::Body => Color::BLACK,
                Tone::Dim => Color::srgb(0.2, 0.2, 0.2),
                Tone::Accent => Color::WHITE,
            }
        }

        fn body_size(&self) -> f32 {
            14.0
        }

        fn small_size(&self) -> f32 {
            11.0
        }
    }

    impl MotionTokens for Test {
        fn motion(&self, _: Motion) -> Curve {
            Curve {
                duration: Duration::from_millis(100),
                ease: |t| t,
            }
        }
    }

    const BLACK: Color = Color::BLACK;
    const GREY: Color = Color::srgb(0.5, 0.5, 0.5);

    /// Halfway from black to white.
    fn halfway() -> Color {
        Color::lerp(&BLACK, &Color::WHITE, 0.5)
    }

    #[derive(Component)]
    struct Selected;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            FynixProtoPlugin::<Test>::default(),
        ))
        .insert_resource(Theme(Test))
        .insert_resource(
            TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(50),
            ),
        );
        // The first update only starts the clock.
        app.update();
        app
    }

    fn color(app: &App, node: Entity) -> Color {
        app.world().get::<TextColor>(node).expect("a label").0
    }

    fn accent(snapshot: &mut LabelSnapshot, theme: &Test) {
        snapshot.color = theme.tone(Tone::Accent);
    }

    fn hover(app: &mut App, node: Entity, on: bool) {
        let mut node = app.world_mut().entity_mut(node);
        if on {
            node.insert(Hovered);
        } else {
            node.remove::<Hovered>();
        }
    }

    #[test]
    fn a_state_rule_applies_while_its_state_holds() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x").when::<Hovered>(accent),
        );
        assert_eq!(color(&app, node), BLACK);

        hover(&mut app, node, true);
        app.update();
        assert_eq!(color(&app, node), Color::WHITE);

        hover(&mut app, node, false);
        app.update();
        assert_eq!(color(&app, node), BLACK);
    }

    #[test]
    fn an_app_can_use_its_own_state_component() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x").when::<Selected>(accent),
        );

        app.world_mut().entity_mut(node).insert(Selected);
        app.update();

        assert_eq!(color(&app, node), Color::WHITE);
    }

    #[test]
    fn rules_apply_in_order_and_only_for_their_state() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x").when::<Hovered>(accent).when::<Pressed>(
                |s: &mut LabelSnapshot, _: &Test| {
                    s.color = GREY;
                },
            ),
        );

        hover(&mut app, node, true);
        app.update();
        assert_eq!(color(&app, node), Color::WHITE);

        app.world_mut().entity_mut(node).insert(Pressed);
        app.update();
        assert_eq!(color(&app, node), GREY);
    }

    #[test]
    fn a_state_rule_beats_the_call_site_and_a_binding() {
        let mut app = app();
        let fixed = mount::<Test>(
            app.world_mut(),
            label("x").tone(Tone::Dim).when::<Hovered>(accent),
        );
        let bound = mount::<Test>(
            app.world_mut(),
            label("x")
                .tone(derived(|_| Tone::Dim))
                .when::<Hovered>(accent),
        );
        hover(&mut app, fixed, true);
        hover(&mut app, bound, true);
        app.update();

        assert_eq!(color(&app, fixed), Color::WHITE);
        assert_eq!(color(&app, bound), Color::WHITE);
    }

    #[test]
    fn a_leaf_with_a_state_rule_stays_mounted() {
        let mut app = app();
        mount::<Test>(
            app.world_mut(),
            label("x").when::<Hovered>(accent),
        );
        mount::<Test>(app.world_mut(), label("y"));

        assert_eq!(app.world().resource::<Mounted<Test>>().len(), 1);
    }

    #[test]
    fn set_rules_reach_the_stateful_leaf_and_its_bare_leaf() {
        let mut app = app();
        let root = mount::<Test>(
            app.world_mut(),
            AnyView::<Bevy, Test>::new(|cx| {
                let root = cx.spawn();
                cx.under(root, |cx| {
                    cx.set::<Label>(|l, _| l.size(20.0));
                    cx.set::<Stateful<Label, Test>>(|l, _| {
                        l.when::<Hovered>(accent)
                    });
                    cx.build(label("x").when::<Pressed>(
                        |s: &mut LabelSnapshot, _: &Test| {
                            s.color = GREY;
                        },
                    ));
                    cx.build(label("plain"));
                });
                root
            }),
        );
        let [stateful, plain] = app
            .world()
            .get::<Children>(root)
            .expect("two labels")
            .iter()
            .collect::<Vec<_>>()[..]
        else {
            panic!("two labels");
        };
        hover(&mut app, stateful, true);
        hover(&mut app, plain, true);
        app.update();

        assert_eq!(color(&app, stateful), Color::WHITE);
        assert_eq!(color(&app, plain), BLACK);
        let size =
            app.world().get::<TextFont>(stateful).unwrap().font_size;
        assert_eq!(size, FontSize::Px(20.0));

        app.world_mut().entity_mut(stateful).insert(Pressed);
        app.update();
        assert_eq!(
            color(&app, stateful),
            GREY,
            "call site rules come last"
        );
    }

    #[test]
    fn nothing_animates_unless_asked() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x").when::<Hovered>(accent),
        );

        hover(&mut app, node, true);
        app.update();

        assert_eq!(color(&app, node), Color::WHITE);
    }

    #[test]
    fn a_transition_reaches_its_target_over_the_curve() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x")
                .when::<Hovered>(accent)
                .transition(Motion::Interact),
        );

        hover(&mut app, node, true);
        app.update();
        assert_eq!(color(&app, node), halfway(), "50ms of 100ms");

        app.update();
        assert_eq!(color(&app, node), Color::WHITE);

        app.update();
        assert_eq!(color(&app, node), Color::WHITE, "and stays");
    }

    #[test]
    fn a_transition_follows_a_bound_prop() {
        let mut app = app();
        app.insert_resource(Selection(Tone::Body));
        let node = mount::<Test>(
            app.world_mut(),
            label("x")
                .tone(derived(|world| {
                    world.resource::<Selection>().0
                }))
                .transition(Motion::Interact),
        );

        app.world_mut().resource_mut::<Selection>().0 = Tone::Accent;
        app.update();

        assert_eq!(color(&app, node), halfway());
    }

    #[derive(Resource)]
    struct Selection(Tone);

    #[test]
    fn an_interrupted_transition_starts_from_where_it_got_to() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x")
                .when::<Hovered>(accent)
                .transition(Motion::Interact),
        );

        hover(&mut app, node, true);
        app.update();
        assert_eq!(color(&app, node), halfway());

        hover(&mut app, node, false);
        app.update();
        assert_eq!(
            color(&app, node),
            Color::lerp(&halfway(), &BLACK, 0.5),
            "halfway back to black from where it was, not from white"
        );

        app.update();
        assert_eq!(color(&app, node), BLACK);
    }

    #[test]
    fn a_transition_can_come_from_a_set_rule() {
        let mut app = app();
        let root = mount::<Test>(
            app.world_mut(),
            AnyView::<Bevy, Test>::new(|cx| {
                let root = cx.spawn();
                cx.under(root, |cx| {
                    cx.set::<Stateful<Label, Test>>(|l, _| {
                        l.transition(Motion::Interact)
                    });
                    cx.build(label("x").when::<Hovered>(accent));
                });
                root
            }),
        );
        let node = app.world().get::<Children>(root).unwrap()[0];

        hover(&mut app, node, true);
        app.update();

        assert_eq!(color(&app, node), halfway());
    }

    #[test]
    fn reduced_motion_snaps() {
        let mut app = app();
        app.insert_resource(ReducedMotion(true));
        let node = mount::<Test>(
            app.world_mut(),
            label("x")
                .when::<Hovered>(accent)
                .transition(Motion::Interact),
        );

        hover(&mut app, node, true);
        app.update();

        assert_eq!(color(&app, node), Color::WHITE);
    }

    #[test]
    fn reduced_motion_finishes_a_transition_under_way() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x")
                .when::<Hovered>(accent)
                .transition(Motion::Interact),
        );
        hover(&mut app, node, true);
        app.update();
        assert_eq!(color(&app, node), halfway());

        app.insert_resource(ReducedMotion(true));
        app.update();

        assert_eq!(color(&app, node), Color::WHITE);
    }

    #[test]
    fn a_despawned_stateful_leaf_is_dropped() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x")
                .when::<Hovered>(accent)
                .transition(Motion::Interact),
        );
        assert_eq!(app.world().resource::<Mounted<Test>>().len(), 1);

        app.world_mut().despawn(node);
        app.update();

        assert!(app.world().resource::<Mounted<Test>>().is_empty());
    }
}
