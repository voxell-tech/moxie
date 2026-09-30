//! Interaction states as components on a node, and the rules and
//! transitions an element attaches to them.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::lifecycle::{Insert, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Query;
use bevy::ecs::system::{Commands, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{Pointer, Press, Release};
use bevy::picking::hover::Hovered as PickingHovered;
use fynix_proto::{Curve, Motion, MotionTokens, Tween};
use motiongfx_interp::interpolation::{InterpFn, Interpolation};

use crate::transition::BevyMarker;
use crate::{Bevy, Cx, Element, Styled};

/// The pointer is over the node or one of its descendants.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Hovered;

/// A pointer button is down on the node.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Pressed;

/// The copy of Bevy's own hover component onto [`Hovered`]. Bevy
/// counts a hovered descendant as hovering its ancestors, so moving
/// between a node and its children never lets go of it.
fn sync_hover(
    insert: On<Insert, PickingHovered>,
    picked: Query<&PickingHovered>,
    mut commands: Commands,
) {
    let node = insert.event_target();
    let Ok(picked) = picked.get(node) else {
        return;
    };
    if picked.get() {
        commands.entity(node).insert(Hovered);
    } else {
        commands.entity(node).remove::<(Hovered, Pressed)>();
    }
}

fn press(press: On<Pointer<Press>>, mut commands: Commands) {
    commands.entity(press.event_target()).insert(Pressed);
}

fn release(release: On<Pointer<Release>>, mut commands: Commands) {
    commands.entity(release.event_target()).remove::<Pressed>();
}

/// The nodes whose state components were inserted or removed since
/// the last update.
#[derive(Resource, Default, Debug)]
pub struct DirtyNodes(pub Vec<Entity>);

fn state_inserted<S: Component>(
    insert: On<Insert, S>,
    mut dirty: ResMut<DirtyNodes>,
) {
    dirty.0.push(insert.event_target());
}

fn state_removed<S: Component>(
    remove: On<Remove, S>,
    mut dirty: ResMut<DirtyNodes>,
) {
    dirty.0.push(remove.event_target());
}

/// Queues the node whenever the state component `S` is inserted on
/// or removed from it.
fn watch_state<S: Component>(world: &mut World, node: Entity) {
    world
        .entity_mut(node)
        .observe(state_inserted::<S>)
        .observe(state_removed::<S>);
}

/// An edit of a snapshot, with the theme in hand.
type Edit<S, T> = Box<dyn Fn(&mut S, &T) + Send + Sync>;

/// An edit that holds while a state component is on the node.
struct StateRule<S, T> {
    holds: fn(&World, Entity) -> bool,
    watch: fn(&mut World, Entity),
    apply: Edit<S, T>,
}

/// A transition to attach, waiting for the theme to name its curve.
struct Move<S, T> {
    motion: Motion,
    curve: fn(&T, Motion) -> Curve,
    interp: InterpFn<S>,
}

impl<S, T> Clone for Move<S, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S, T> Copy for Move<S, T> {}

/// An element with state rules and a transition attached.
///
/// It is an [`Element`] itself, so it mounts like any other, and it is
/// [`Styled`], so set rules for `Stateful<E, T>` can add rules to
/// every such element in a scope.
pub struct Stateful<E: Element<Bevy, T>, T> {
    element: E,
    rules: Vec<StateRule<E::Snapshot, T>>,
    motion: Option<Move<E::Snapshot, T>>,
}

impl<E: Element<Bevy, T>, T: Send + Sync + 'static> Stateful<E, T> {
    fn new(element: E) -> Self {
        Self {
            element,
            rules: Vec::new(),
            motion: None,
        }
    }

    /// While the component `S` is on the node, `rule` edits the
    /// resolved values. Rules apply in the order added, after the
    /// call-site values and any bound props.
    pub fn when<S: Component>(
        mut self,
        rule: impl Fn(&mut E::Snapshot, &T) + Send + Sync + 'static,
    ) -> Self {
        self.rules.push(StateRule {
            holds: |world, node| world.entity(node).contains::<S>(),
            watch: watch_state::<S>,
            apply: Box::new(rule),
        });
        self
    }

    /// Makes every change to the written values travel over the
    /// theme's curve for `motion`.
    pub fn transition(mut self, motion: Motion) -> Self
    where
        T: MotionTokens,
        E::Snapshot: Interpolation<BevyMarker>,
    {
        self.motion = Some(Move {
            motion,
            curve: T::motion,
            interp:
                <E::Snapshot as Interpolation<BevyMarker>>::interp,
        });
        self
    }
}

/// What any element can be given state rules and a transition with.
pub trait StateExt<T: Send + Sync + 'static>:
    Element<Bevy, T>
{
    /// This element with a rule for the state `S`.
    fn when<S: Component>(
        self,
        rule: impl Fn(&mut Self::Snapshot, &T) + Send + Sync + 'static,
    ) -> Stateful<Self, T> {
        Stateful::new(self).when::<S>(rule)
    }

    /// This element with a transition.
    fn transition(self, motion: Motion) -> Stateful<Self, T>
    where
        T: MotionTokens,
        Self::Snapshot: Interpolation<BevyMarker>,
    {
        Stateful::new(self).transition(motion)
    }
}

impl<T: Send + Sync + 'static, E: Element<Bevy, T>> StateExt<T>
    for E
{
}

impl<T: Send + Sync + 'static, E: Element<Bevy, T>> Styled
    for Stateful<E, T>
{
    fn unset() -> Self {
        Self::new(E::unset())
    }

    fn over(mut self, below: Self) -> Self {
        let mut rules = below.rules;
        rules.append(&mut self.rules);
        Self {
            element: self.element.over(below.element),
            rules,
            motion: self.motion.or(below.motion),
        }
    }
}

impl<T: Send + Sync + 'static, E: Element<Bevy, T>> Element<Bevy, T>
    for Stateful<E, T>
{
    type Snapshot = E::Snapshot;

    fn prepare(world: &mut World, node: Entity) {
        E::prepare(world, node);
        world
            .entity_mut(node)
            .insert(PickingHovered::default())
            .observe(sync_hover)
            .observe(press)
            .observe(release);
    }

    fn snapshot(&self, world: &World, theme: &T) -> E::Snapshot {
        self.element.snapshot(world, theme)
    }

    fn write(
        snapshot: &E::Snapshot,
        world: &mut World,
        node: Entity,
    ) {
        E::write(snapshot, world, node);
    }

    fn is_live(&self) -> bool {
        self.element.is_live() || !self.rules.is_empty()
    }

    fn changed(&mut self, world: &World) -> bool {
        self.element.changed(world)
    }

    fn on_mounted(&self, world: &mut World, node: Entity) {
        self.element.on_mounted(world, node);
        for rule in &self.rules {
            (rule.watch)(world, node);
        }
    }

    fn resolve(self, cx: &Cx<'_, Bevy, T>) -> Self {
        let Self {
            element,
            rules,
            motion,
        } = self;
        // Rules for the bare element, then rules for the wrapped one.
        let element = E::resolve(element, cx);
        cx.resolve(Self {
            element,
            rules,
            motion,
        })
    }

    fn adjust(
        &self,
        snapshot: &mut E::Snapshot,
        world: &World,
        node: Entity,
        theme: &T,
    ) {
        self.element.adjust(snapshot, world, node, theme);
        for rule in &self.rules {
            if (rule.holds)(world, node) {
                (rule.apply)(snapshot, theme);
            }
        }
    }

    fn tween(&self, theme: &T) -> Option<Tween<E::Snapshot>> {
        match self.motion {
            Some(motion) => Some(Tween {
                curve: (motion.curve)(theme, motion.motion),
                interp: motion.interp,
            }),
            None => self.element.tween(theme),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use bevy::app::App;
    use bevy::color::Color;
    use bevy::ecs::hierarchy::Children;
    use bevy::ecs::relationship::RelationshipTarget;
    use bevy::ecs::resource::Resource;
    use bevy::text::{FontSize, TextColor, TextFont};
    use bevy::time::{TimePlugin, TimeUpdateStrategy};

    use super::*;
    use crate::mounted::Mounts;
    use crate::tokens::{TextTokens, Tone};
    use crate::transition::ReducedMotion;
    use crate::views::{Label, LabelSnapshot, label};
    use crate::{
        AnyView, FynixProtoPlugin, Theme, derived, every_frame,
        mount, resource,
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
        blend(&BLACK, &Color::WHITE, 0.5)
    }

    fn blend(from: &Color, to: &Color, t: f32) -> Color {
        <Color as Interpolation<BevyMarker>>::interp(from, to, t)
    }

    #[derive(Component)]
    struct Selected;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
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
                .tone(derived(|_| Tone::Dim).when(every_frame()))
                .when::<Hovered>(accent),
        );
        hover(&mut app, fixed, true);
        hover(&mut app, bound, true);
        app.update();

        assert_eq!(color(&app, fixed), Color::WHITE);
        assert_eq!(color(&app, bound), Color::WHITE);
    }

    #[test]
    fn an_element_with_a_state_rule_stays_mounted() {
        let mut app = app();
        mount::<Test>(
            app.world_mut(),
            label("x").when::<Hovered>(accent),
        );
        mount::<Test>(app.world_mut(), label("y"));

        assert_eq!(app.world().resource::<Mounts<Test>>().len(), 1);
    }

    #[test]
    fn set_rules_reach_the_stateful_element_and_its_bare_element() {
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
                .tone(resource::<Selection, _>(|selection| {
                    selection.0
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
            blend(&halfway(), &BLACK, 0.5),
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
    fn inserting_and_removing_a_state_re_applies_its_rule() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x").when::<Selected>(accent),
        );

        app.world_mut().entity_mut(node).insert(Selected);
        assert_eq!(
            app.world().resource::<DirtyNodes>().0,
            vec![node],
            "the insert observer queued the node"
        );
        app.update();
        assert_eq!(color(&app, node), Color::WHITE);
        assert!(app.world().resource::<DirtyNodes>().0.is_empty());

        app.world_mut().entity_mut(node).remove::<Selected>();
        assert_eq!(
            app.world().resource::<DirtyNodes>().0,
            vec![node]
        );
        app.update();
        assert_eq!(color(&app, node), BLACK);
    }

    #[test]
    fn a_state_the_element_has_no_rule_for_does_not_queue_it() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x").when::<Hovered>(accent),
        );

        app.world_mut().entity_mut(node).insert(Selected);

        assert!(app.world().resource::<DirtyNodes>().0.is_empty());
    }

    #[test]
    fn a_despawned_stateful_element_is_dropped() {
        let mut app = app();
        let node = mount::<Test>(
            app.world_mut(),
            label("x")
                .when::<Hovered>(accent)
                .transition(Motion::Interact),
        );
        assert_eq!(app.world().resource::<Mounts<Test>>().len(), 1);

        app.world_mut().despawn(node);
        app.update();

        assert!(app.world().resource::<Mounts<Test>>().is_empty());
    }
}
