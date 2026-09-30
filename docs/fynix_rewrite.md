# A fynix rewrite

Status: design. Nothing here is built yet. The plan is to prove it in a
throwaway prototype crate before touching `vendor/fynix`, which is
vendored as a submodule for when that time comes.

## Why

Today's fynix has three problems that keep coming back:

1. **Elements are tied to one theme.** The theme is part of the host
   type (`BevyHost<EditorTheme>`), `#[element]` writes impls for that
   one host, every `Style` and `field_patch!` names it, and element
   defaults read `EditorTheme`'s fields directly (about 90 reads across
   16 of the 23 element files). No other app can reuse them.
2. **Child elements bake structure into types.** `Button` can only
   hold an `icon` and a `label`. Reaching them takes machinery that
   exists for nothing else: child records, patch and despawn walking,
   `Style::finish`, and `.bind(|b| b.label().text(), ..)` paths.
3. **Composers hide their parts and copy their fields.** `FieldRow`
   re-declares `label`, `color` and `bold` for its inner `Label`.
   Behaviour on an inner part needs a callback like `Foldable`'s
   `on_header: FnOnce(ElementMut<Button>)`, and a style cannot target a
   composer at all.

And one smaller one: transitions only fire on tag changes, their timing
is fixed per element type, and nothing animates in, out, or through
layout.

## Principles

1. **A widget is a struct that owns its own props and holds other
   views whole.** It never copies another view's fields.
2. **Children are views passed in**, not typed slots.
3. **Styling flows down; nothing reaches in.** Parents style
   descendants through scoped rules, never by editing a child after
   it is built.
4. **Everything a caller wants is said before the build.** Values,
   bindings, behaviour and style. Composites hand nothing back.
5. **One precedence order**, the same for every value, with states
   layered on top.
6. **Nothing happens by default that a caller did not ask for**,
   animation included.

## Prior art

None of this is original. Each piece has shipped somewhere:

| Idea | From |
|---|---|
| Views as structs with a body, modifiers, an environment flowing down | SwiftUI |
| A generic `View` trait, tuples of child views, type erasure at boundaries | Xilem |
| Styling through builder methods on the element struct | GPUI (Zed) |
| A prop that is a value or a live signal, fine-grained updates | Leptos, SolidJS |
| `Cx`, derived values, `when` and `each` over the Bevy ECS | Quill |
| One modifier chain that applies to any widget | Jetpack Compose |
| A per-widget theme trait (`Catalog`), backend behind a trait | iced |
| Scoped set rules and show rules | Typst |
| One cascade, states as pseudo-classes | CSS |

Their known problems come with them: Xilem's type bloat and compile
times, the runtime cost of SwiftUI's environment, and Quill having been
set aside in favour of Bevy's own `bsn!` direction. Both Quill's history
and Xilem's current type erasure are worth reading before building.

## Backend boundary

Generic above the elements; the backend owns elements, layout, input and
state access. This is the line fynix's `Host` already draws.

```rust
pub trait Backend {
    type World;
    type Node: Copy;
    fn spawn(world: &mut Self::World, parent: Self::Node) -> Self::Node;
}

pub trait View<B: Backend, T> {
    fn build(self, cx: &mut Cx<B, T>) -> B::Node;
}
```

`T` is the app's theme. `Cx` carries it, so a view or a rule can read
tokens from it through a trait bound (see [Theming](#theming)), and
`T` is inferred everywhere a `Cx` is in hand.

A composite only arranges other views, so it works on every backend
unchanged. An element is written once per backend. On Bevy that leaves
layout, picking, text and focus to `bevy_ui`, taffy and bevy's picking.

The heavier alternative, iced's, is for the framework to own layout,
events and focus, with backends only drawing primitives. It would mean
rebuilding what `bevy_ui` gives us and running a second UI system next
to the ECS. Not worth it unless these widgets need to run outside
Bevy.

## Views

Everything is a struct. Functions are only short constructors:
`label("Open")` and `Label::new("Open")` are the same thing.

### Elements

Own fields and a build hook, and nothing else:

```rust
pub struct Label {
    pub text: Prop<String>,
    pub size: Prop<f32>,
    pub tone: Prop<Tone>,
    pub wrap: Prop<bool>,
}

// Any theme that can answer for text and motion.
impl<T: TextTokens + MotionTokens> View<Bevy, T> for Label {
    fn build(self, cx: &mut Cx<Bevy, T>) -> Entity {
        // spawn Text, then bind each prop; an unset one falls back to
        // cx.theme().body_size() and friends
    }
}

pub fn label(text: impl Into<Prop<String>>) -> Label { /* ... */ }

impl Label {
    pub fn size(mut self, size: impl Into<Prop<f32>>) -> Self {
        self.size = size.into();
        self
    }
}
```

### Composite views

Their own props, plus whole views as generic fields. The struct says
what the widget is; `build` says what it is made of.

```rust
pub struct FieldRow<L, V> {
    pub label: L,    // any view, styled by the caller
    pub value: V,    // any view
    pub depth: u32,  // FieldRow's own prop
}

impl<B, T, L, V> View<B, T> for FieldRow<L, V>
where
    B: Backend,
    L: View<B, T>,
    V: View<B, T>,
{
    fn build(self, cx: &mut Cx<B, T>) -> B::Node {
        cx.scope(|cx| {
            // Its layout needs the label on one line. A caller's own
            // explicit `.wrap(true)` still wins.
            cx.set(Label::wrap(false));
            row((
                self.label.width(pct(40)),
                self.value.grow(1.0),
            ))
            .gap(8.0)
            .build(cx)
        })
    }
}
```

A new field on `Label` works inside `FieldRow` without touching it.

### Modifiers

Two forms:

- A method setting the struct's own field: `label.size(12.0)`.
- A generic wrapper, for modifiers any view takes:

```rust
pub struct Padded<V> { inner: V, padding: UiRect }

pub trait ViewExt<B: Backend, T>: View<B, T> + Sized {
    fn padding(self, padding: UiRect) -> Padded<Self> {
        Padded { inner: self, padding }
    }
}
```

A modifier on a composite applies to its root node:
`field_row(..).padding(4.0)` pads the row.

### No handles back

`ui.add(view)` returns nothing. Today composers return an
`ElementHandle`, and callbacks hand out `ElementMut`s, only because a
caller can attach behaviour after the build. That leaks internals,
fights the borrow on `ui`, goes stale on a rebuild, and lets a caller
undo a composite's own rules. Everything a caller wants is said before
the build instead:

```rust
let header = button(label(name))
    .on_activate(move |world| select(world, entity))
    .draggable(drag::row(entity))
    .context_menu(row_menu(entity));

ui.add(foldable(header, body));
```

The one escape hatch is imperative access after build: focus, scroll
into view, measure. The caller makes a ref and passes it in:

```rust
let search = NodeRef::new();
ui.add(text_input(query).node_ref(&search));
// later, in an observer or a system:
search.focus(world);
```

## Props and reactivity

```rust
pub enum Prop<T> {
    Value(T),
    Bound(Signal<T>),
}

label(derived(move |world| name_of(world, entity)))
```

A binding is given where the prop is set, instead of attached
afterwards through a path. Structure changes through:

- `when(signal, view)`: build `view` while `signal` holds.
- `each(list, key, view)`: one view per item, matched by `key`, so a
  row keeps its fold state, focus and transitions when the list
  reorders.

Change sources are the ones `moxie_ui::reactive` already has: resource
and component ticks, projections, and plain value comparison.

Text inputs need one explicit rule: a bound value never overwrites a
focused input. Today that is special-cased in `NumberField`'s patch.

## Styling

Styling is Typst's model: set rules, show rules, and explicit arguments
winning over both.

### Set rules

A set rule changes a view's default for one field, from where it is
set to the end of its scope. Inner scopes override outer ones.

```rust
// App-wide, like a Typst preamble.
ui.set(Label::size(12.0));
ui.set(Button::fill(fill));

ui.scope(|ui| {
    ui.set(Label::tone(Tone::Dim));
    ui.add(field_row(label("Subject"), value)); // dim
});

// Conditional, like `#set ... if cond`.
ui.set_if(compact, Button::height(18.0));
```

`Label::size(12.0)` is a field path plus a value. fynix already mints a
typed `FieldId` path per field for `.bind(..)`, so a set rule is a
field path and a value kept on a stack of scopes.

### Show rules

A show rule modifies every view of a kind in scope. It may add
modifiers and state rules; it may not replace the view with a
different one, since bindings and state on the original would have
nowhere to land.

```rust
ui.show::<Label>(|label| label.when(Hovered, |l| l.tone(Tone::Accent)));

// A selector, like Typst's `heading.where(level: 1)`.
ui.show::<Button>()
    .within::<Menu>()
    .apply(|button| button.radius(0.0));
```

### Rule bundles

Named styles such as `GhostButton` become bundles of rules, applied to
one view or to a whole scope:

```rust
fn ghost<T: SurfaceTokens>() -> Rules<Button, T> {
    Rules::new()
        .set(Button::fill(Color::NONE))
        .set(Button::transition(Button::FILL, Motion::Interact))
        .when(Hovered, Button::fill_with(|theme: &T| theme.hover()))
}

button(content).rules(ghost());
ui.scope_with(ghost(), |ui| { /* every Button here */ });
```

Styling a composite's parts needs nothing extra: they are ordinary
views, so ordinary rules reach them. `Style::finish`, which exists
only to reach children after the call site built them, goes away.

### State rules

Conditional values on top of everything else, the way CSS
pseudo-classes are:

```rust
label(name).when(Hovered, |l| l.tone(Tone::Accent))
```

A state rule is the one way something changes a value it does not
own. `FieldName`'s draggable label adds `when(HasAction, ..)` rules on
top of the caller's tone instead of binding the colour a second time,
so the two never fight over the field.

This replaces today's anim `read` fields: `hover_color`,
`dragged_color`, `Label::lit()` and `Label::dragged()` all go, along
with the backlog item about letting `read` return an `Option`.

#### State rules are scoped set rules

A state rule is a block of set rules that holds while a node is in a
state, and reaches everything built under that node:

```rust
button(row((icon(save), label("Save"))))
    .when::<Hovered, _>(|cx: &mut Cx<Bevy, Theme>| {
        cx.set::<Label>(|l, _| l.tone(Tone::Accent));
        cx.set::<Icon>(|i, _| i.tone(Tone::Accent));
    })
    .when::<Pressed, _>(|cx: &mut Cx<Bevy, Theme>| {
        cx.root(|cx| cx.set::<Frame>(|f, t| f.fill(t.panel())));
    })
    .transition(Motion::Interact)
```

This is CSS's `.button:hover .label`, or a Typst `set` with a condition.
A composite's parts are ordinary views, so a rule reaches them without
the composite forwarding anything, even when a part is a generic `C`
the composite knows nothing about. Rules for one part among several of
the same type go through `lenz` paths
(`cx.set_field(Card::cursor().title().tone(), ..)`).

An element has the one-rule form, sugar for a block with one set rule
for itself:

```rust
label(name).when::<Hovered, Theme>(|l, _| l.tone(Tone::Accent))
```

Three more kinds of rule block make the rest of a composite's styling
rules too:

- `cx.root(..)`: rules that reach only the root of the view they are
  set for, the first node spawned after them. A button's pressed fill
  above would otherwise reach a row nested in its content.
- `cx.defaults(..)`: rules weaker than any that are not, wherever they
  are. This is the composite-default layer finding 1 asked for.
- `cx.transition(motion)`: a rule naming the curve for every element in
  scope whose snapshot says how it blends. `.transition(motion)` on
  any view is a scope holding one.

So `Button` is only a frame, its content and two behaviours, and all
its defaults, hover included, are root defaults a theme or call site
can beat:

```rust
fn defaults<T: SurfaceTokens + SpacingTokens + MotionTokens>(
    cx: &mut Cx<Bevy, T>,
) {
    cx.set::<Frame>(|f, t| f.fill(t.fill()).radius(t.radius()));
    cx.when::<State<Hovered>>(|cx| {
        cx.set::<Frame>(|f, t| f.fill(t.hover()));
    });
    cx.transition(Motion::Interact);
}

// In Button::build, before building the frame and then the content:
cx.defaults(|cx| cx.root(defaults));
```

Underneath:

1. `.when::<S, _>(block)` opens a scope and runs the block with its
   rules marked by the condition: a `holds` and a `watch` function for
   `S`. Each rule entry also records the first node spawned after it,
   which is the root of the view `.when` was written on.
2. An element resolved while that root is not yet spawned is the root
   itself, and one resolved after it is a descendant. For each
   conditional rule reaching it, the element runs the rule on an unset
   view once, at build, and keeps the result as a layer: the props the
   rule sets, the node the state is read on, and whether it is its own.
3. Mounting a layered element calls `watch` on that node, which in
   Bevy adds insert and remove observers once per node and state, and
   records the element as a reader of the node. A state change marks
   the node and all its readers dirty. The refcount that keeps rules
   alive across `keyed` rebuilds keeps these alive too, so a child
   rebuilt under the node picks them up through its capture.
4. A dirty element swaps the props of each layer whose state holds
   into itself, takes a snapshot, and swaps them back. Nothing is
   cloned, so bound props in a layer work. A layer written on an
   ancestor only swaps in props the call site left unset.

A rule acts on props, not on the resolved snapshot, so the element
resolves `l.tone(Accent)` through the theme like any other value.

The swap needs each element to number its props, which the `styled!`
macro writes along with `unset` and `over` from a list of fields. A
field enumeration in `lenz`'s derive could replace it.

### Precedence

From weakest to strongest, for every value:

| Layer | Example |
|---|---|
| View default | A neutral constant |
| Composite defaults, outer then inner | `cx.defaults(..)` |
| Set rules, outer scope then inner | `ui.set(Label::size(12.0))` |
| State rules from ancestors, outer then inner | `row(..).when(Hovered, ..)` |
| Call-site argument | `label("x").size(20.0)` |
| State rules on the node itself, inner then outer | `.when(Hovered, ..)` |

A state rule that reaches into descendants ranks just above set rules,
not above the call site. `label("Save").tone(Tone::Dim)` inside a
hovered button stays dim: an explicit value is how a part opts out.

State rules on the node itself run the other way: the outer one wins.
It is the one written later in `.when(a).when(b)`, and the one a call
site wrote around a composite's own. Composite defaults are weaker
within each state rank too.

This settles the "composite constraints" question: a composite's needs
are set rules in its own scope, and a caller's explicit argument still
wins. That is Typst's behaviour too.

## Theming

A view states what it needs from a theme as a trait bound, and works
under any theme that satisfies it. Set rules then style on top. Both
read the same tokens, so they are one mechanism, not two.

### Token traits

Small traits, each covering one concern, read through methods so a
theme can compute or store its values however it likes:

```rust
pub trait TextTokens {
    fn text(&self) -> Color;
    fn text_dim(&self) -> Color;
    fn body_size(&self) -> f32;
    fn small_size(&self) -> f32;
}

pub trait MotionTokens {
    fn motion(&self, kind: Motion) -> Curve; // Interact, Expand, ...
}
```

From what moxie_ui's elements read today, roughly six cover them all:

| Trait | Covers | Read by |
|---|---|---|
| `TextTokens` | text colours, text sizes | labels, icons, buttons |
| `MotionTokens` | named motions, reduced motion | every transition |
| `SurfaceTokens` | fill, hover, panel, hairline, accent, critical | buttons, frames, panels, tabs |
| `SpacingTokens` | `xs` to `xl`, radius, row, touch, hairline | layout defaults |
| `MenuTokens` | menu radius, padding, item radius, layer | dropdowns, menu surfaces |
| `TimelineTokens` | clip colours, playhead | timeline views |

The exact split is settled while porting, keeping each trait to what
several views share. An app implements them once for its own theme:

```rust
impl TextTokens for EditorTheme {
    fn text(&self) -> Color { self.color.text }
    fn text_dim(&self) -> Color { self.color.text_dim }
    fn body_size(&self) -> f32 { self.text.body }
    fn small_size(&self) -> f32 { self.text.small }
}
```

### Where tokens are read

- **View defaults.** An element bounded on `TextTokens` falls back to
  `theme.body_size()` for a size nobody set, so it looks right with no
  rules at all. A view that needs nothing from the theme takes no
  bound, and any theme can use it.
- **Rule bundles and preambles.** `ghost::<T: SurfaceTokens>()` above
  reads `theme.hover()`. A preamble is a function of the theme that
  sets rules for a whole app:

```rust
pub fn compact<T: SpacingTokens>(ui: &mut Ui<T>) {
    ui.set(Button::height_with(|theme: &T| theme.row()));
    ui.set(Label::size(11.0));
}
```

A view's bound only names what its defaults read. Rules may read any
token the theme has, since they are written by the app that owns the
theme.

### Why the theme rides on `Cx`, not on the backend

Today the theme is part of the host type, `BevyHost<EditorTheme>`, and
`Style` names its host as an associated type. Two things go wrong
there, and both shape this design:

- `impl<T: SurfaceTokens> Style for GhostButton` cannot compile: `T`
  appears nowhere Rust can pin it down (E0207). Styles have to be
  generic over the theme themselves.
- Keying a style by host (`Style<H>`) fails one step later. Deferred
  element values like `elem!(!GhostButton)` are only handed `&Theme`,
  and Rust cannot infer a host from its theme type, since many hosts
  could share one.

Here the theme is its own parameter, `Cx<B, T>`, and rules are
`Rules<V, T>`. `T` is always known from the `Cx` a view is built with,
so nothing has to infer it backwards.

### What stays tied to the app

`FynixBuild`, `BevyUi`, `BevyFynix`, the reactive predicates and the
editor's own composites name `EditorTheme` directly. They are the
app's plumbing. Only shared views, their rules and their tokens are
generic.

Feathers' `UiTheme`, which still colours `NumberField`'s input and the
dropdown popup, is separate; see "Unify theming onto `EditorTheme`" in
`backlog.md`.

### Open questions

- Where the token traits live: beside the views, or in a small crate
  other view libraries can depend on.
- Whether a token trait may have default methods built from others
  (`text_dim` from `text`), so a small theme implements less.

## Transitions

A transition is a styling property like any other, and nothing
animates unless a rule says so.

```rust
ui.set(Button::transition(Button::FILL, Motion::Interact));
button(content).fill(highlight).transition(Button::FILL, Motion::Expand);
```

- **It follows the resolved value.** Whatever moves it animates the
  same way: a state rule, a binding, a set rule, a new call-site
  value. Today only tag changes do; a bound selection highlight
  snaps.
- **Named motions come from the theme's `MotionTokens`**, not repeated
  per field: `Motion::Interact` resolves to one curve for the whole
  app.

- **Only interpolable types can transition.** Colours, opacity and
  sizes can; text, numbers being typed, enums and `Val::Auto` snap.
- **Interruption** starts from wherever the value had got to, as today.
  A spring curve carries velocity, so a reversal mid-transition is
  smooth, which today's `min(duration, spent)` only approximates.
- **Enter and exit.** See below.
- **Reduced motion.** One token scales every curve to zero.
- **Layout transitions, later.** FLIP: measure a node before and after
  layout, animate the difference as a transform. It needs a hook after
  bevy's layout pass and runs one frame late, so it comes last.

### Props every element shares

Opacity and scale belong to every element, so rules for every kind of
element reach them: `cx.set::<Visual>(|v, _| v.opacity(0.5))`. Each
element also has them as its own props (`label(x).opacity(0.5)`), and
lends them to the core through `Element::visual`, so `Visual` rules and
state rules resolve into the same fields. A rule for the element's own
kind beats a `Visual` rule.

In Bevy, opacity multiplies the alpha of what the element draws, and
scale is its `UiTransform`. The two behave differently in a subtree:
Bevy has no opacity a node's children inherit, so fading a subtree
means a rule reaching every element in it, while a scale on the root
already scales its children. A rule that should only touch the root
goes in `cx.root(..)`.

### Entering and leaving

Entering and leaving are two more states, which the framework sets
itself, and they are styled like any other:

```rust
line(name)
    .when::<Entering, _>(hidden)
    .when::<Leaving, _>(hidden)
    .transition(Motion::Expand)
// The same, shorter:
line(name).appear(hidden).transition(Motion::Expand)

fn hidden<T>(cx: &mut Cx<Bevy, T>) {
    cx.set::<Visual>(|v, _| v.opacity(0.0));
    cx.root(|cx| cx.set::<Visual>(|v, _| v.scale(0.9)));
}
```

- **Entering** is put on a view's root when a rule reading it is
  built, so the first write already has it. It comes off after that
  update, which marks the view dirty, and the transition runs to the
  resting values from the next update on. Views with no rule for it
  never get it.
- **Leaving** is put on the root of a view that `keyed` or `each`
  drops, if its root element has a transition. The view stays where it
  was, stops taking input, and its `Leaving` rules animate it out over
  that curve. After the same length again, the space it took collapses
  (`Backend::collapse`) and then it is despawned. A view with no
  transition is despawned at once, and reduced motion drops every
  leaving view at the next update.
- **The collapse** measures the node, pins its size along the parent's
  main axis, clips it, zeroes its padding and minimum size, and shrinks
  it to nothing. It pulls the parent's gap beside it back with a
  negative margin, so nothing jumps when it goes. While it runs, the
  node's own element leaves its size to the collapse.
- **Order.** `each` keeps a leaving row after the row it followed, so
  other rows move around it until it is gone. `keyed` builds the new
  view beside the leaving one.

Only views fynix removes itself can animate out. An entity despawned
from outside the UI vanishes at once. A leaving view's timing is two
lengths of its root element's curve, fade then collapse, not a wait on
the transitions inside it.

## Three real call sites

### Asset field button (`moxie_ui/src/inspector/handle.rs`)

Today:

```rust
let mut slot = ui.elem(elem!(
    !GhostButton,
    width = percent(100),
    justify = JustifyContent::SpaceBetween,
    icon = elem!(Icon, image = icons::ASSET, color = muted),
    label = elem!(Label, text = label, color = muted, wrap = false)
));
slot.bind(|button| button.label().text(), changed, read_label)
    .observe(open_picker);
```

Rewritten:

```rust
ui.add(
    button(row((
        icon(ASSET),
        label(derived(move |world| label_of::<A>(world, &*read)))
            .wrap(false),
    )))
    .rules(ghost())
    .tone(Tone::Dim)
    .width(pct(100))
    .justify(SpaceBetween)
    .on_activate(open_picker),
);
```

### Hierarchy row (`moxie/src/ui/hierarchy.rs`)

Today: `Foldable` holding a `GhostButton` holding a `Label`, with an
`on_header` callback that binds the fill, the label's text and the
label's colour after the build.

Rewritten:

```rust
let name = derived(move |world| name_of(world, entity));
let unnamed = derived(move |world| is_unnamed(world, entity));

ui.add(
    foldable(
        button(
            label(name)
                .wrap(false)
                .when(unnamed, |l| l.tone(Tone::Dim)),
        )
        .rules(ghost())
        .height(18.0)
        .fill(derived(move |world| highlight(world, entity)))
        .on_activate(move |world| select(world, entity))
        .draggable(drag::row(entity))
        .context_menu(row_menu(entity)),
        move || subtree(children_of(entity)),
    )
    .folds_on(FoldsOn::Chevron)
    .open(folded_state(entity)),
);
```

`foldable` builds `row((chevron, header))` itself, so it never needs
to know the header has an icon slot.

### Field row (`moxie_ui/src/inspector.rs`)

Today `FieldRow` copies `label`, `color` and `bold` from `Label`, and
switches internally between a plain label and `FieldName`.

Rewritten:

```rust
ui.add(field_row(label("Subject").tone(Tone::Dim), value).depth(0));

// An animatable field: the same label, with a modifier.
ui.add(field_row(label("translation").animatable(field), value));
```

## Strain points

What to prove in the prototype first, most likely to fail first:

1. **Type size and compile time.** Nested generic views grow long
   concrete types; Xilem has hit slow builds and unreadable errors.
   Erasing at composite boundaries (an `AnyView`) means `Box<dyn
   View>`, which the code convention asks to confirm first.
2. **Rule and environment cost.** Resolving each field against a
   stack of scoped rules on every build must be cached per scope and
   view type, not walked per read.
3. **Rules that change later.** A theme switch or a compact-mode
   toggle must re-resolve exactly the affected nodes. That needs the
   scope stack kept per node and change-tracked.
4. **Tracing a value.** "Why is this label 14pt?" now has several
   answers. We need a way to show which layer won, like browser
   devtools do for CSS.
5. **Text inputs.** The focused-input rule under
   [Props and reactivity](#props-and-reactivity).
6. **Exits against rebuilds**, under [Transitions](#transitions).
7. **Escape hatches.** The number field's scrub and the timeline's
   drag previews still need direct ECS access, as custom elements
   with a build hook, roughly how today's fynix elements work.

## Prototype findings

The prototype is split the way fynix is:

- `crates/fynix_proto` is the core, `no_std` like fynix, with no Bevy
  anywhere in its dependency tree: `typarena`, `lenz`,
  `motiongfx_interp` and `hashbrown`. It builds for a bare-metal
  target (`thumbv7em-none-eabihf`).
  Rules live in typarena tables, one row per scope depth and one
  column per view kind, so leaving a scope is one `remove_row`. Live
  elements get one column per element kind, each walked by an update
  registered the first time that kind mounts, as fynix's `AnimTable`
  does. Transitions use `motiongfx_interp`'s `EaseFn` and `InterpFn`.
  18 tests run it against a fake backend that is only a list of nodes.
- `crates/bevy_fynix_proto` is the Bevy backend: the elements,
  composites, states, modifiers, the three call sites and the two
  measurement examples, with 59 tests at the split. It depends on the
  `bevy` crate with its defaults off and three features
  (`bevy_picking`, `bevy_ui_widgets`, `bevy_window`), like the rest of
  the workspace. That is about half the tree of `bevy` with its
  defaults on (1436 lines of `cargo tree`). An earlier version listed
  eleven Bevy sub-crates instead; the tree came out the same (701
  lines against 720 now, the difference being `bevy_window`), so the
  umbrella crate won on one version to pin and `bevy::` import paths.

It started as one crate building the core, `Label`, `Frame`, `row`,
`column`, `Icon`, `Button`, generic modifiers, state rules and opt-in
transitions, with 40 headless tests. What held up:

- Set rules as the view's own builder methods
  (`cx.set::<Label>(|l, theme| l.size(theme.small_size()))`), merged
  by a per-view `over` that fills only what the call site left unset.
  Typst's precedence with no macro.
- One `Label` built under two unrelated theme types through token
  bounds.
- Bound props, and state rules beating the call site.
- Transitions that start from wherever an interrupted one had got to,
  and a reduced-motion switch.

What did not, each needing a decision before the real rewrite:

1. **A composite's defaults beat the app's rules.** `Button` sets its
   fill as a scoped set rule, and an inner rule wins, so an app-wide
   `set::<Frame>(fill)` cannot restyle buttons. The precedence order
   needs a "composite default" layer below every set rule.
2. **Modifier names collide with styling methods.**
   `row(..).padding(4.0)` hits the row's own frame padding, not the
   generic padding modifier. Either composites drop their forwarding
   methods and rely on modifiers, or modifiers get distinct names.
3. **Two writers on one node.** An element rewrites its components when a
   bound prop changes, and can overwrite what a modifier set. The
   prototype only writes props that were set, by convention. Modifiers
   also run once, at build, so they cannot be bound.
4. **Composites are only as generic as their elements.** `Button` holds a
   `Frame`, a Bevy element, so `Button` is Bevy-only. The `FieldRow<B>`
   example above overstates this: backend-generic composites need
   backend-generic building blocks, or are simply Bevy composites.
5. **State rules act on resolved values.** They edit the element's
   snapshot (`s.color = theme.tone(Accent)`), because a `Prop` holding
   a boxed closure cannot be cloned per frame. So a rule cannot say
   `l.tone(Accent)` and have the element resolve it. Acting on props needs
   cloneable props, which means shared ownership of bound closures.
6. **Transitions are whole-snapshot.** An element cannot animate its colour
   and snap its size. Per-field transitions need per-field keys and a
   field-by-field diff.
7. **A continuously changing target never settles.** A bound value
   that moves every frame restarts its transition every frame, lags,
   then snaps. Driven values need a follow mode, or no transition.
8. **Everything live is polled.** Each frame, every element with a bound
   prop or a state rule re-reads its snapshot and compares it, and a
   element with a state rule stays live forever. Change detection on the
   sources would replace the poll.
9. **Rules for stateful elements name the theme type.** Adding state
   rules through a set rule means
   `set::<Stateful<Label, Theme>>(..)`, and rule closures often need
   their `&Theme` parameter annotated.

The three call sites are built in `src/demo/`. Each got shorter than
today's code: the asset button from about 45 lines to 17, and the
hierarchy row loses its `on_header` callback and change predicates
entirely. `FieldRow` no longer copies `Label`'s fields. They also
turned up five more problems:

10. **A composite's inner parts cannot take state rules.** `Button`
    holds a bare `Frame`, so a hover or selection fill cannot be added
    from a call site, and the `ghost()` bundle above is not expressible.
    State rules need to reach a composite's own elements.
11. **Bound values cannot read the theme.** A signal only gets
    `&World`, and the theme is out of the world while views update. A
    selection fill had to be passed in as a colour. Signals need the
    theme too, or should return tokens (`Tone::Accent`) the element
    resolves.
12. **State rules only watch the element's own node.** `.when::<S>` takes a
    component on that node, not a signal, so the doc's
    `.when(unnamed, ..)` became a bound tone instead. Conditions from
    elsewhere in the world need signal-driven states.
13. **Structure is eager.** `foldable`'s body is built up front and
    hidden, not built when first opened, and its open state cannot be
    one prop shared by the chevron, the body and the caller.
14. **Handlers are awkward to own.** An activate handler is not
    `Clone`, so stacking several on one node needs bookkeeping in a
    component. Tests also need a frame for handlers queued through
    `Commands` to run.

Growing a hover label and lighting a button's fill turned up one more
problem and answered another in part:

15. **Animatable properties every element shares have no home.** A
    transform (and later opacity) belongs to every element, but it had
    to be added to `Label` alone, as a `scale` prop written to
    `UiTransform`. Generic modifiers are build-once and cannot take
    state rules or transitions (finding 3), so they cannot carry it.
    The design needs a place for properties all elements share, so a
    `when::<Hovered>` rule can animate them on any of them.

Finding 10 is partly answered. `Button` now wires a hover rule onto its
own frame while building it, with a `hover_fill` prop and a transition
from the theme, so a composite can put state rules on its own parts. A
call site still cannot add one, and `hover_fill` is read once at
build, so it cannot be bound.

`hover_fill` is the wrong shape: a hover colour is a transition
between two rule sets, not a prop. Scoped state rules (see "State rules
are scoped set rules" above) are now in the prototype, and `Stateful`,
`hover_fill` and snapshot-editing rules are gone. They answer:

- finding 1, through `cx.defaults`: an app's `set::<Frame>` now
  restyles every button;
- finding 5: rules act on props, by swapping a layer's props in and
  out rather than cloning them;
- finding 9: nothing names `Stateful<Label, Theme>` any more;
- finding 10: a call site's `.when` reaches a composite's parts, and
  one rule block can light a button's frame and every label in it.

Findings 6 and 15 remained: transitions were still whole-snapshot, and
properties every element shares still needed a home. Building it
turned up three more:

16. **The one-rule form does not chain.** `label(x).when::<A, _>(a)`
    returns a wrapper, not a `Label`, so a second `.when` is the block
    form and takes `own(b)` instead of `b`. Keeping the element's type
    through its rules would need the element to hold them, generic over
    the theme.
17. **Rule blocks need their context type spelled out.** A block is a
    closure called later with `&mut Cx<Bevy, T>`, so it must be
    annotated (`|cx: &mut Cx<Bevy, Theme>|`), or the theme type named
    in `.when::<S, Theme>`. The gallery aliases it as `Build`.
18. **A set rule can no longer give every element of a kind a state
    rule.** `set::<Stateful<Label, T>>(|l| l.when(..))` did that. The
    same is now a `.when` block on an ancestor, which reads the
    ancestor's state, not each element's own.

Entering and leaving (see "Entering and leaving" above) answer finding
15 with `Visual`, and turned up four more:

19. **No opacity a subtree inherits.** Bevy draws each node's colours
    as they are, so fading a subtree takes a rule reaching every
    element in it, and overlapping translucent children do not fade as
    one layer. A frame's border is not faded either, since `Frame`
    does not own its border colour.
20. **`keyed` and `each` containers are rows.** A container is a bare
    node, which lays its children out left to right, so a leaving
    view's collapse takes its width, and a switched screen appears
    beside the old one until it goes. Containers need a direction, or
    a frame of their own.
21. **A leaving view's timing is a guess.** It fades for one length of
    its root element's curve and collapses for another, instead of
    waiting for the transitions under it to settle, which would need
    `Mounted` to know which elements are under which node.
22. **A returning key does not revive its leaving view.** A row
    removed and added back while it leaves is built fresh beside the
    old one.

The hover source changed too. The `Hovered` marker used to be set by
`Pointer<Over>` and `Pointer<Out>` observers on the node. Both events
bubble, so a pointer crossing from a button's padding onto its label
fires Out and Over on the button in turn, and the marker drops and
returns, taking `Pressed` with it. Bevy 0.19 has a hover component for
this in `bevy::picking::hover`: `Hovered(bool)` is true while the
pointer is over the entity or any descendant, and it only changes when
that changes. A stateful element now carries it, and an observer copies
each change onto the prototype's marker. There is also `DirectlyHovered`,
which leaves descendants out, and `HoverMap`, which is per frame and
per pointer. Both are less fit for this. Bevy's component is kept up to
date by its picking plugins, so an app without them never sees a hover. The
tests drive it through a `HoverMap` and `update_is_hovered`, not a real
pointer.

Composites and custom elements (`Foldable`, its private `Reveal` element)
needed no core changes, which suggests the escape hatch for custom
views works.

### Generic against boxed views

`bevy_fynix_proto/examples/screen_generic.rs` and `screen_boxed.rs` build the
same editor-like screen: 405 nodes, depth 10, 4 panels, 24 distinct
sibling types, a stateful label with a transition in most rows. The
boxed one erases each row, button and panel with `AnyView`. A 6x copy
(2431 nodes) was measured too, from a scratch file. Times are the
median of 3, in seconds, on an Apple M5 Max, over a baseline of 0.99
that is mostly linking Bevy:

| | Generic | Boxed | 6x generic | 6x boxed |
|---|---|---|---|---|
| Clean debug build | 1.30 | 1.27 | 1.74 | 1.84 |
| Clean release build | 1.23 | 1.21 | 1.80 | 2.19 |
| Rebuild after a one-line edit | 0.86 | 0.91 | 1.42 | 1.70 |
| Top-level `type_name` length | 16,132 | 378 | 95,110 | 532 |

What it says:

- **Strain point 1 is not a build-time problem at this scale.** rustc
  checks a 95k-character type in well under a second.
- **Boxing does not speed builds up.** Each boxed view is still a
  compiled closure, and at 6x boxing everything is slower, by 20% in
  release.
- **Boxing buys readability.** Type names shrink 40 to 180 times. A
  bad tuple element reads `(AnyView, AnyView, u32, AnyView)` instead
  of burying `u32` under a 4,000-character line of type soup.
- **So erase at boundaries, not everywhere:** at panels, and wherever a
  function hands a view to another module.

Two smaller things turned up. `.boxed()` often needs its backend and
theme spelled out (`<_ as ViewExt<Bevy, Editor>>::boxed(..)`), and six
children per tuple forces real screens to nest tuples, so `ViewSeq`
needs more arities. The split core takes tuples of up to twelve.

### What the split found

Porting the Bevy side onto the core needed no core changes beyond a
rename, but four things were awkward:

- **`derived` cannot infer its world.** The core's `derived` is generic
  over the world type, so a closure reading `world.resource::<X>()`
  does not type-check without it. Each backend needs its own thin
  `derived`. Worth documenting as part of writing a backend.
- **Snapshot interpolation is written by hand.** Each snapshot needs an
  `Interpolation` impl calling each field's own, with a turbofish per
  field since `f32` and `Color` use different markers. It is verbose,
  and two elements skip it entirely. A derive, or a helper for "blend
  these fields, snap the rest", would fix it.
- **`Mounted` needs a newtype to be a Bevy resource**, by the orphan
  rule, with `Deref` and `Default` boilerplate.
- **The core's per-update settings were called `Frame`**, clashing with
  the `Frame` view. Renamed to `Tick`.

It also turned up a bug outside the prototype: vendored
`motiongfx_interp` did not build without `std`, since its integer
interpolation called `f64::round`. The rest of the workspace never saw
it because `bevy_motiongfx` turns `std` on. Upstream fixed it the same
way, through `libm`, in motiongfx #187, which is on `main` but not yet
in a release.

### Set rules through `lenz` paths

`cx.set_field(Text::cursor().size(), 20.0)` sets one field by its
`#[derive(Lenz)]` path, and `set_field_with` reads the value from the
theme. It sits beside the closure form, `cx.set::<Text>(|t, _|
t.size(20.0))`, and resolves the same way: a path rule becomes a
closure over the path's accessor, so precedence does not change.

What a path gives that a closure cannot:

- **It reaches a composite's own parts.**
  `set_field(Card::cursor().title().size(), 30.0)` sizes every card's
  title, and not its body or a loose `Text`. This answers finding 10
  for set rules. State rules still cannot reach parts.
- **It says which field it sets**, so a value can be traced. A path
  rule records its field and scope; `cx.trace::<Text>(field)` returns
  the depth of every path rule naming that field, and counts the
  closure rules that might also have set it, since nothing can see
  inside one. That is strain point 4. It is also an argument for
  making paths the main way to write a rule, with closures as the
  escape hatch.

What it costs, or showed:

- **A composite part rule acts like the part's call site.** The card
  sets its title's size before building it, so a `Text` rule no
  longer reaches that title. That is consistent (the card is the
  title's call site), but it means the more specific rule wins by
  structure, not by being set later.
- **The derive needs public types.** `#[derive(Lenz)]` emits public
  cursor traits, so a view and every type in its fields must be
  `pub`, and fields that are not props need `#[lenz(ignore)]`.
- **Values must be `Clone`,** since a rule applies once per view built.
  A bound prop cannot be set through a path, for the same reason a
  bound prop cannot be shared: finding 5 again.
- **Composites generic over the theme can clash with elements.** In the
  core's tests, `impl<T> View<Fake, T> for Card` overlaps the core's
  `View` for every `Element`, because another crate could make `Card` an
  `Element<Fake, ItsTheme>`. `Card` had to name one theme. The Bevy
  backend's composites are generic over the theme and do not hit it,
  so the exact rule needs working out before the real rewrite; one
  way out is for elements to get `View` from a derive rather than a
  blanket impl.

Still unmeasured: pointer-driven states against a real pointer (the
tests toggle them directly), and the heap cost of boxing.

## Migration

1. **Prototype.** A throwaway crate with `View`, `Prop`, `Cx`, set and
   show rules, state rules and one transition, on Bevy. It builds the
   three call sites above and measures strain points 1 to 3.
2. **Decide** the token-trait split and whether `AnyView` erasure is
   acceptable, from what the prototype shows.
3. **Rewrite fynix** on its own branch in `vendor/fynix`, keeping the
   parts that carry over:

   | Today | Becomes |
   |---|---|
   | `bind`, `watch` | Bound props, `when`, `each` |
   | Tags and `anim(on(..))` | State rules and transitions |
   | `lenz` field paths | Set-rule targets |
   | `Build` hooks | Custom elements |
   | `Host`, with the theme as its associated type | `Backend`, with the theme on `Cx<B, T>` |
   | Element defaults reading `EditorTheme` fields | Defaults through token-trait bounds |
   | `#[elem(child)]` | Views passed in |
   | `Style`, `Style::finish` | Rule bundles |
   | Composers returning handles | Composites returning nothing, plus `NodeRef` |

4. **Port moxie_ui one element at a time**, then the composites, then the
   editor's call sites, keeping CI green at each step. `EditorTheme`
   implements the token traits as the first step of the port.
