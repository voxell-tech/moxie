//! Generic pieces `bevy_fynix` lacks, kept free of moxie concepts.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::prelude::*;
use bevy_fynix::patch::Paint;
use bevy_fynix::tokens::{
    Motion, MotionTokens, SpacingTokens, SurfaceTokens, TextTokens,
    Tone,
};
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, Icon, Open, button, frame,
    icon, tint,
};
use bevy_fynix::{
    Bevy, Cx, Prop, ScopedExt as _, Signal, View, ViewExt as _,
    component, keyed,
};
use fynix::Transition;

/// Upstream: a signal that re-reads at every update and fires when
/// the value differs from the last one.
///
/// For state with no tick to watch. `read` runs twice per update, so
/// keep it cheap.
pub fn changing<T>(
    read: impl Fn(&World) -> T + Clone + Send + Sync + 'static,
) -> Signal<T>
where
    T: PartialEq + Send + Sync + 'static,
{
    let mut seen: Option<T> = None;
    let peek = read.clone();
    Signal::new(read, move |world: &World| {
        let current = peek(world);
        let fired = seen.as_ref() != Some(&current);
        seen = Some(current);
        fired
    })
}

/// Upstream: a signal of a projection of the resource `R`, firing
/// only when the projection differs, whatever else in `R` changed.
///
/// `R` has to be in the world whenever the signal is read.
pub fn projection<R: Resource, K>(
    project: impl Fn(&R) -> K + Clone + Send + Sync + 'static,
) -> Signal<K>
where
    K: PartialEq + Send + Sync + 'static,
{
    changing(move |world: &World| project(world.resource::<R>()))
}

/// Upstream: a signal of the current value of the state `S`, `None`
/// while the state is not initialised.
pub fn state_of<S: States>() -> Signal<Option<S>> {
    changing(|world: &World| {
        world.get_resource::<State<S>>().map(|s| s.get().clone())
    })
}

/// Upstream: a view whose root node, a label or an icon, is drawn in
/// a colour of its own rather than a [`Tone`].
///
/// For a colour no tone names, like an axis's red. The colour is
/// kept in the node's paint, so a later repaint writes it back.
pub struct Inked<V> {
    inner: V,
    color: Color,
}

/// `inner`, drawn in `color`.
pub fn inked<V>(inner: V, color: Color) -> Inked<V> {
    Inked { inner, color }
}

impl<T, V: View<Bevy, T>> View<Bevy, T> for Inked<V> {
    fn build(self, cx: &mut Cx<'_, Bevy, T>) -> Entity {
        let node = self.inner.build(cx);
        set_ink(cx.world, node, self.color);
        node
    }
}

/// Upstream: draws the label or icon `node` in `color`, whatever its
/// tone says, and keeps the colour in its paint.
pub fn set_ink(world: &mut World, node: Entity, color: Color) {
    let Ok(mut entity) = world.get_entity_mut(node) else {
        return;
    };
    if let Some(mut paint) = entity.get_mut::<Paint>() {
        paint.ink = color;
    }
    if let Some(mut text) = entity.get_mut::<TextColor>() {
        text.0 = color;
    }
    if let Some(mut image) = entity.get_mut::<ImageNode>() {
        image.color = color;
    }
}

/// Upstream: the width of a [`Rail`]'s line.
pub const RAIL_WIDTH: f32 = 1.0;

/// Upstream: a body set in under a vertical line, as under a fold's
/// header.
///
/// The line is `inset` from the left edge, and the body sits `step`
/// right of it. The line is the theme's hairline unless
/// [`color`](Self::color) says.
pub struct Rail<B> {
    inset: f32,
    step: f32,
    color: Option<Color>,
    display: Option<Prop<Display>>,
    body: B,
}

/// A [`Rail`] around `body`.
pub fn rail<B>(inset: f32, step: f32, body: B) -> Rail<B> {
    Rail {
        inset,
        step,
        color: None,
        display: None,
        body,
    }
}

impl<B> Rail<B> {
    /// The line's colour.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Whether the whole rail is shown. `Display::None` takes it out
    /// of the layout.
    pub fn display(
        mut self,
        display: impl Into<Prop<Display>>,
    ) -> Self {
        self.display = Some(display.into());
        self
    }
}

impl<T, B> View<Bevy, T> for Rail<B>
where
    T: SurfaceTokens + SpacingTokens + Send + Sync + 'static,
    B: View<Bevy, T>,
{
    fn build(self, cx: &mut Cx<'_, Bevy, T>) -> Entity {
        let color =
            self.color.unwrap_or_else(|| cx.theme().hairline());
        let mut outer = frame()
            .direction(FlexDirection::Row)
            .width(percent(100.0))
            .align(AlignItems::Stretch)
            .gap(0.0)
            .padding(UiRect::left(px(self.inset)));
        if let Some(display) = self.display {
            outer = outer.display(display);
        }
        let node = cx.build(outer);
        cx.under(node, |cx| {
            cx.build(frame().width(px(RAIL_WIDTH)).fill(color));
            let content = cx.build(
                frame()
                    .direction(FlexDirection::Column)
                    .grow(1.0)
                    .gap(0.0)
                    .padding(UiRect::left(px(self.step))),
            );
            cx.under(content, |cx| self.body.build(cx));
        });
        node
    }
}

/// Upstream: what a click has to land on to fold a [`Fold`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldOn {
    /// The header, which has to be activatable, such as a button.
    Header,
    /// A chevron button of the fold's own beside the header, leaving
    /// the header free to mean something else.
    Chevron,
}

/// Upstream: the icon that turns with a [`Fold`]'s state.
///
/// Handed to the header closure, which may place [`icon`](Self::icon)
/// in the header it builds.
#[derive(Clone)]
pub struct Chevron {
    image: Handle<Image>,
    size: Option<f32>,
    tone: Tone,
    shut: f32,
    open: f32,
    state: Option<Entity>,
}

impl Chevron {
    /// A chevron drawn from `image`, at `shut` degrees while the fold
    /// is shut and `open` while it is open.
    pub fn new(image: Handle<Image>, shut: f32, open: f32) -> Self {
        Self {
            image,
            size: None,
            tone: Tone::Dim,
            shut,
            open,
            state: None,
        }
    }

    /// The length of the icon's sides.
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    /// The icon's tint.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// The icon, turning over the theme's interact transition with
    /// the fold's state. Fixed at the shut angle while the fold has
    /// nothing to fold.
    pub fn icon(&self) -> Transition<Icon> {
        let mut icon = icon(self.image.clone())
            .tone(self.tone)
            .rotation(self.rotation());
        if let Some(size) = self.size {
            icon = icon.size(size);
        }
        icon.transition(Motion::Interact)
    }

    fn rotation(&self) -> Prop<f32> {
        let (shut, open) = (self.shut, self.open);
        match self.state {
            Some(state) => component::<Open, _>(state, move |on| {
                if on.is_some() { open } else { shut }
            })
            .into(),
            None => shut.into(),
        }
    }
}

/// Flips the [`Open`] on `state` and reports the result.
fn flip(
    world: &mut World,
    state: Entity,
    on_toggle: &impl Fn(&mut World, bool),
) {
    let Ok(mut entity) = world.get_entity_mut(state) else {
        return;
    };
    let open = if entity.contains::<Open>() {
        entity.remove::<Open>();
        false
    } else {
        entity.insert(Open);
        true
    };
    on_toggle(world, open);
}

/// Upstream: a header over a body that is built while the fold is
/// open and dropped while it is shut, set in under a [`Rail`].
///
/// Its state is [`Open`] on the fold's root node, or on the entity
/// given to [`state_on`](Self::state_on), where anything may set it.
/// The header is built by a closure that gets the [`Chevron`]; what
/// folds is [`FoldOn`].
pub struct Fold<H, B, F = fn(&mut World, bool)> {
    chevron: Chevron,
    header: H,
    body: B,
    on: FoldOn,
    enabled: bool,
    open: bool,
    state: Option<Entity>,
    on_toggle: F,
    toggle: f32,
    step: f32,
    rail: Option<Color>,
}

/// A [`Fold`] of `header` over `body`, open, folded by its header.
///
/// `header` is called once with the [`Chevron`]. `body` is called
/// each time the fold opens.
pub fn fold<H, B>(
    chevron: Chevron,
    header: H,
    body: B,
) -> Fold<H, B> {
    Fold {
        chevron,
        header,
        body,
        on: FoldOn::Header,
        enabled: true,
        open: true,
        state: None,
        on_toggle: |_, _| {},
        toggle: 14.0,
        step: 9.0,
        rail: None,
    }
}

impl<H, B, F> Fold<H, B, F> {
    /// What folds it.
    pub fn on(mut self, on: FoldOn) -> Self {
        self.on = on;
        self
    }

    /// Whether there is anything to fold. A fold with nothing in it
    /// neither toggles nor turns, and has no chevron button of its
    /// own.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether it starts open. Ignored when the state is on another
    /// entity.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Keeps the state as [`Open`] on `entity`, whatever its own
    /// lifetime, instead of on the fold's root.
    pub fn state_on(mut self, entity: Entity) -> Self {
        self.state = Some(entity);
        self
    }

    /// The chevron's image, for a caller that has none at hand when
    /// it makes the fold.
    pub fn image(mut self, image: Handle<Image>) -> Self {
        self.chevron.image = image;
        self
    }

    /// The side of the chevron button, which also sets the rail in
    /// by half of it, and how far the body is set in from the rail.
    pub fn layout(mut self, toggle: f32, step: f32) -> Self {
        self.toggle = toggle;
        self.step = step;
        self
    }

    /// The colour of the rail's line.
    pub fn rail_color(mut self, color: Color) -> Self {
        self.rail = Some(color);
        self
    }

    /// Calls `on_toggle` with the new state each time a click flips
    /// it, so the caller can keep its own copy.
    pub fn on_toggle<G>(self, on_toggle: G) -> Fold<H, B, G>
    where
        G: Fn(&mut World, bool) + Send + Sync + 'static,
    {
        Fold {
            chevron: self.chevron,
            header: self.header,
            body: self.body,
            on: self.on,
            enabled: self.enabled,
            open: self.open,
            state: self.state,
            on_toggle,
            toggle: self.toggle,
            step: self.step,
            rail: self.rail,
        }
    }
}

impl<T, H, HV, B, BV, F> View<Bevy, T> for Fold<H, B, F>
where
    T: TextTokens
        + SurfaceTokens
        + SpacingTokens
        + MotionTokens
        + Send
        + Sync
        + 'static,
    H: FnOnce(Chevron) -> HV,
    HV: View<Bevy, T>,
    B: Fn() -> BV + Send + Sync + 'static,
    BV: View<Bevy, T> + 'static,
    F: Fn(&mut World, bool) + Send + Sync + 'static,
{
    fn build(self, cx: &mut Cx<'_, Bevy, T>) -> Entity {
        let root = cx.build(
            frame()
                .direction(FlexDirection::Column)
                .width(percent(100.0))
                .gap(2.0),
        );
        let state = self.state.unwrap_or(root);
        if self.state.is_none() && self.open {
            cx.world.entity_mut(root).insert(Open);
        }
        let mut chevron = self.chevron;
        if self.enabled {
            chevron.state = Some(state);
        }
        let folds = self.enabled;
        let on_toggle = self.on_toggle;
        let toggle = self.toggle;

        cx.under(root, |cx| {
            let head = cx.build(
                frame()
                    .direction(FlexDirection::Row)
                    .width(percent(100.0))
                    .align(AlignItems::Center)
                    .gap(0.0),
            );
            cx.under(head, |cx| {
                let header = (self.header)(chevron.clone());
                if folds && self.on == FoldOn::Chevron {
                    cx.build(
                        button(chevron.icon())
                            .width(px(toggle))
                            .height(px(toggle))
                            .radius(3.0)
                            .rules(tint)
                            .on_activate(move |world| {
                                flip(world, state, &on_toggle);
                            }),
                    );
                    let slot = cx.build(frame().grow(1.0));
                    cx.under(slot, |cx| cx.build(header));
                } else if folds {
                    let slot = cx.build(frame().grow(1.0));
                    cx.under(slot, |cx| {
                        cx.build(header.on_activate(move |world| {
                            flip(world, state, &on_toggle);
                        }))
                    });
                } else {
                    let slot = cx.build(frame().grow(1.0));
                    cx.under(slot, |cx| cx.build(header));
                }
            });

            let body = self.body;
            let shown = keyed::<T, bool>(
                component::<Open, _>(state, |on| on.is_some()),
                move |open| {
                    if *open {
                        body().boxed()
                    } else {
                        frame().boxed()
                    }
                },
            )
            .within(
                frame().direction(FlexDirection::Column).gap(0.0),
            );
            let mut rail = rail(self.toggle / 2.0, self.step, shown)
                .display(component::<Open, _>(state, |on| {
                    if on.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    }
                }));
            if let Some(color) = self.rail {
                rail = rail.color(color);
            }
            cx.build(rail)
        });
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource)]
    struct Pair(u32, u32);

    #[test]
    fn changing_fires_on_first_read_and_on_difference() {
        let mut world = World::new();
        world.insert_resource(Pair(1, 1));
        let mut signal =
            changing(|world: &World| world.resource::<Pair>().0);

        assert!(signal.changed(&world));
        assert!(!signal.changed(&world));
        world.resource_mut::<Pair>().0 = 2;
        assert!(signal.changed(&world));
        assert_eq!(signal.get(&world), 2);
    }

    #[test]
    fn projection_ignores_other_fields() {
        let mut world = World::new();
        world.insert_resource(Pair(1, 1));
        let mut signal = projection::<Pair, _>(|pair| pair.0);

        assert!(signal.changed(&world));
        world.resource_mut::<Pair>().1 = 5;
        assert!(!signal.changed(&world));
        world.resource_mut::<Pair>().0 = 5;
        assert!(signal.changed(&world));
    }
}
