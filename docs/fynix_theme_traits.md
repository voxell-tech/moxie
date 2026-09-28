# Theme traits for fynix elements

Status: proposed. Work happens on `nixon/theme-traits`, with fynix
vendored as a submodule at `vendor/fynix` (on a local branch, not
pushed upstream until decided).

## Problem

Every element in `moxie_ui` is tied to one concrete theme,
`EditorTheme`, so no other app can reuse them.

The tie runs through three layers:

1. fynix's `Host` has an associated `type Theme`, and bevy_fynix's
   host is `BevyHost<Theme>`. `moxie_ui` fixes it to
   `FynixHost = BevyHost<EditorTheme>`.
2. `#[element]` writes its impls for one host, `crate::FynixHost` by
   default, so every element is only `Element<BevyHost<EditorTheme>>`.
   Every `Style` names that host as its associated `type Host`, and
   every `field_patch!` writer is a `FieldPatch<FynixHost>`.
3. Element defaults, animation legs and build hooks read the struct's
   fields directly: `#[elem(default = theme.text.body)]`,
   `anim(duration = theme.motion.interact)`,
   `build.theme.color.panel`. That is about 90 reads across 16 of the
   23 element files.

`Label` needs a text size, a colour and a transition. It should work
under any theme that can answer for those.

## Goal

Each element states what it needs from a theme as a trait bound, and
works under any theme that satisfies it. The traits are small and
shared between elements. An app gets an element by implementing its
traits for its own theme, however that theme stores its values.

Call sites do not change: `ui.elem(elem!(Label, text = "Save"))`
already fixes the host, and the theme type is inferred from it.

## Design

### Theme traits

Small traits, each covering one concern, read through methods so a
theme can compute or store its values however it likes:

```rust
pub trait TextTheme {
    fn text(&self) -> Color;
    fn text_dim(&self) -> Color;
    fn body_size(&self) -> f32;
    fn small_size(&self) -> f32;
}

pub trait MotionTheme {
    fn interact(&self) -> Duration;
    fn ease(&self) -> EaseFn;
}
```

From what the elements read today, roughly six cover them all:

| Trait | Covers | Read by |
|---|---|---|
| `TextTheme` | text colours, text sizes | `Label`, `Icon`, buttons |
| `MotionTheme` | transition duration and ease | every animated field |
| `SurfaceTheme` | fill, hover, panel, hairline, accent, critical | buttons, frames, panels, tabs |
| `SpacingTheme` | `xs` to `xl`, radius, row, touch, hairline | layout defaults |
| `MenuTheme` | menu radius, padding, item radius, layer | dropdown, menu surface |
| `TimelineTheme` | clip colours, playhead | timeline elements |

The exact split is settled while converting the elements, keeping each
trait to what several elements share.

An app implements them once:

```rust
impl TextTheme for EditorTheme {
    fn text(&self) -> Color { self.color.text }
    fn text_dim(&self) -> Color { self.color.text_dim }
    fn body_size(&self) -> f32 { self.text.body }
    fn small_size(&self) -> f32 { self.text.small }
}
```

### Elements state their bound

`#[element]` gains a `theme = <bounds>` form:

```rust
#[element(theme = TextTheme + MotionTheme, build = Self::build)]
pub struct Label {
    #[elem(default = theme.body_size(), patch = PatchTextSize)]
    pub size: f32,
    #[elem(default = ::NONE, patch = PatchTextColor, anim(
        duration = theme.interact(),
        ease = theme.ease(),
        on(Hovered, read = Self::lit),
    ))]
    pub color: Color,
}
```

It expands to impls over any theme meeting the bound, through the
same `crate::FynixHost` path the macro already defaults to, now
generic:

```rust
// In moxie_ui:
pub type FynixHost<T> = bevy_fynix::host::BevyHost<T>;

// What the macro writes:
impl<T> Element<crate::FynixHost<T>> for Label
where
    T: TextTheme + MotionTheme + Send + Sync + 'static,
{ /* ... */ }
```

fynix itself stays backend-agnostic: it never names Bevy, only the
crate's own `FynixHost<T>`. The existing `host = <path>` form keeps
working for elements tied to one host.

Build hooks take the same bound:

```rust
impl Label {
    fn build<T: TextTheme + MotionTheme>(
        &self,
        build: &mut Build<'_, FynixHost<T>, Self>,
    ) { /* ... */ }
}
```

### Patches are theme-free

A `field_patch!` writer only moves a value onto a component, so it
becomes a blanket impl over every theme:

```rust
impl<T: Send + Sync + 'static> FieldPatch<FynixHost<T>> for PatchTextSize
```

### Styles are keyed by theme

`Style` names its host as an associated type today. A style generic
over the theme cannot be written that way:
`impl<T: SurfaceTheme> Style for GhostButton` leaves `T` unconstrained
(E0207).

Keying `Style` by host instead (`Style<H>`) also fails, one step
later. `elem!(!GhostButton)` expands to a closure that is only handed
`&Theme`. Rust cannot infer a host from its theme type, because
`H::Theme` is a projection that many hosts could share.

So `Style` is keyed by the theme type, the way `Seed<Th>` already is:

```rust
pub trait Style<Th> {
    type Element: Seed<Th>;
    fn apply(&self, element: &mut Self::Element, theme: &Th) {}
    fn finish(&self, element: &mut Self::Element, theme: &Th) {}
}

impl<T: SurfaceTheme> Style<T> for GhostButton {
    type Element = Button;
    fn apply(&self, button: &mut Button, theme: &T) {
        button.fill = theme.fill();
    }
}
```

`styled` then starts from `Seed<Th>` rather than `ElementBase<H>`, and
infers `Th` from the theme it is handed.

## What stays tied to the app

`FynixBuild`, `BevyUi`, `BevyFynix`, the reactive predicates and
composers are the app's own plumbing. They keep naming the app's
theme, as `FynixHost<EditorTheme>`. Only elements, their patches and
their styles become generic.

Feathers' `UiTheme`, which still colours `NumberField`'s input and the
dropdown popup, is a separate concern. See "Unify theming onto
`EditorTheme`" in `backlog.md`.

## Plan

In fynix (`vendor/fynix`, local branch):

- [ ] `#[element(theme = <bounds>)]`: generic impls of `ElementBase`,
      `Seed` and `Element` over `crate::FynixHost<T>`, the bounds
      plus `Send + Sync + 'static`.
- [ ] `Style<Th>` keyed by theme, `styled` and `elem!`'s `!style`
      form over `Seed<Th>`.
- [ ] Tests: one element and one style generic over a theme trait,
      used from two different theme types in the same test.

In moxie_ui:

- [ ] Define the theme traits, and implement them for `EditorTheme`.
- [ ] `FynixHost<T>` generic, with the app's own alias for
      `FynixHost<EditorTheme>`.
- [ ] `field_patch!` writes blanket impls.
- [ ] Convert the elements and the five styles (`TintButton`,
      `MenuButton`, `SegmentButton`, `GhostButton`, `MenuSurface`).

Then:

- [ ] Run the full CI set from `.github/workflows/rust.yml`.
- [ ] Decide whether the fynix changes go upstream, and whether the
      theme traits move out of `moxie_ui` into a crate of their own.

## Open questions

- Where the traits live: in `moxie_ui` beside the elements, or in a
  small crate other element libraries can depend on without taking
  `moxie_ui`.
- Whether a trait may have default methods built from others (for
  example `text_dim` from `text`), so a small theme implements less.
- Whether `host = <path>` and `theme = <bounds>` should both stay, or
  the concrete form go once every element is generic.
