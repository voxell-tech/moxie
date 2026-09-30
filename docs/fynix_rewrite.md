# A fynix rewrite

Status: design. Nothing here is built yet. The plan is to prove it in a
throwaway prototype crate before touching `vendor/fynix`.

This supersedes parts of `fynix_theme_traits.md`; see
[Theming](#theming).

## Why

Today's fynix has three problems that keep coming back:

1. **Elements are tied to one theme.** Every element implements
   `Element<BevyHost<EditorTheme>>` and reads `EditorTheme`'s fields
   directly, so no other app can reuse them.
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

Generic above the leaves; the backend owns leaves, layout, input and
state access. This is the line fynix's `Host` already draws.

```rust
pub trait Backend {
    type World;
    type Node: Copy;
    fn spawn(world: &mut Self::World, parent: Self::Node) -> Self::Node;
}

pub trait View<B: Backend> {
    fn build(self, cx: &mut Cx<B>) -> B::Node;
}
```

A composite only arranges other views, so it works on every backend
unchanged. A leaf is written once per backend. On Bevy that leaves
layout, picking, text and focus to `bevy_ui`, taffy and bevy's picking.

The heavier alternative, iced's, is for the framework to own layout,
events and focus, with backends only drawing primitives. It would mean
rebuilding what `bevy_ui` gives us and running a second UI system next
to the ECS. Not worth it unless these widgets need to run outside
Bevy.

## Views

Everything is a struct. Functions are only short constructors:
`label("Open")` and `Label::new("Open")` are the same thing.

### Leaf views

Own fields and a build hook, and nothing else:

```rust
pub struct Label {
    pub text: Prop<String>,
    pub size: Prop<f32>,
    pub tone: Prop<Tone>,
    pub wrap: Prop<bool>,
}

impl View<Bevy> for Label {
    fn build(self, cx: &mut Cx<Bevy>) -> Entity {
        // spawn Text, then bind each prop
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

impl<B: Backend, L: View<B>, V: View<B>> View<B> for FieldRow<L, V> {
    fn build(self, cx: &mut Cx<B>) -> B::Node {
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

pub trait ViewExt<B: Backend>: View<B> + Sized {
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
fn ghost() -> Rules<Button> {
    Rules::new()
        .set(Button::fill(Color::NONE))
        .set(Button::transition(Button::FILL, Motion::Interact))
        .when(Hovered, Button::fill_with(|tokens| tokens.hover()))
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

### Precedence

From weakest to strongest, for every value:

| Layer | Example |
|---|---|
| View default | A neutral constant |
| Set rules, outer scope then inner | `ui.set(Label::size(12.0))` |
| Call-site argument | `label("x").size(20.0)` |
| State rules | `.when(Hovered, ..)` |

This settles the "composite constraints" question: a composite's needs
are set rules in its own scope, and a caller's explicit argument still
wins. That is Typst's behaviour too.

## Theming

With scoped set rules, views barely need to read a theme. They can
have neutral defaults, and a theme becomes a preamble of set rules:

```rust
pub fn monokai(ui: &mut Ui, tokens: &EditorTokens) {
    ui.set(Label::tone_color(Tone::Body, tokens.text));
    ui.set(Label::size(tokens.body));
    ui.set(Button::fill(tokens.fill));
    ui.set(Button::transition(Button::FILL, Motion::Interact));
    // ...
}
```

Open decision, recorded rather than assumed:

- **Views carry no theme bound.** Any app shares them with nothing to
  implement. Tokens and the theme traits only matter to whoever writes
  a preamble.
- **Or views keep the trait bounds from `fynix_theme_traits.md`**, so a
  view looks right with no preamble at all.

The first is close to "option B" rejected earlier. Its real downside
was that every call site needed a style to look right. Scoped set rules
remove that, since one preamble styles the whole app.

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
- **Named motions come from the tokens**, not repeated per field:

```rust
pub trait MotionTokens {
    fn motion(&self, kind: Motion) -> Curve; // Interact, Expand, ...
}
```

- **Only interpolable types can transition.** Colours, opacity and
  sizes can; text, numbers being typed, enums and `Val::Auto` snap.
- **Interruption** starts from wherever the value had got to, as today.
  A spring curve carries velocity, so a reversal mid-transition is
  smooth, which today's `min(duration, spent)` only approximates.
- **Enter and exit.** `.enter(Fade)` and `.exit(Fade)`. An exiting node
  stays alive until its exit ends. It needs keyed `each` to tell a
  node that is really leaving from one that is only rebuilt; a full
  rebuild runs no exit.
- **Reduced motion.** One token scales every curve to zero.
- **Layout transitions, later.** FLIP: measure a node before and after
  layout, animate the difference as a transform. It needs a hook after
  bevy's layout pass and runs one frame late, so it comes last.

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
   drag previews still need direct ECS access, as custom leaf views
   with a build hook, roughly how fynix elements work today.

## Migration

1. **Prototype.** A throwaway crate with `View`, `Prop`, `Cx`, set and
   show rules, state rules and one transition, on Bevy. It builds the
   three call sites above and measures strain points 1 to 3.
2. **Decide** the theming question and whether `AnyView` erasure is
   acceptable, from what the prototype shows.
3. **Rewrite fynix** on its own branch in `vendor/fynix`, keeping the
   parts that carry over:

   | Today | Becomes |
   |---|---|
   | `bind`, `watch` | Bound props, `when`, `each` |
   | Tags and `anim(on(..))` | State rules and transitions |
   | `lenz` field paths | Set-rule targets |
   | `Build` hooks | Custom leaf views |
   | `Host` | `Backend` |
   | `#[elem(child)]` | Views passed in |
   | `Style`, `Style::finish` | Rule bundles |
   | Composers returning handles | Composites returning nothing, plus `NodeRef` |

4. **Port moxie_ui leaf by leaf**, then the composites, then the
   editor's call sites, keeping CI green at each step.

`fynix_theme_traits.md` stays useful for step 2: if views keep theme
bounds, its trait list and macro changes still apply.
