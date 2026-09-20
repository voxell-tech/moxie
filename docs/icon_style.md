# Icon style

Icons live in `assets/icons/<category>/<name>.png`
(`arrows`, `editor`, `files`, `general`, `media`, `shapes`). Paths are
exposed as consts in `crates/moxie/src/icons.rs` (the app's own) and
`crates/moxie_ui/src/icons.rs` (what the dock/inspector engine draws).

## Format

- PNG, 96 x 96, 8-bit RGBA, non-interlaced. No SVG.
- Every glyph is pure white (`#FFFFFF`) on a transparent background.
  Anti-aliased edges are the only partial alpha. The colour comes from
  the `Icon` element's `color` tint (`ImageNode.color`), so a new icon
  must stay white to tint correctly.
- Drawn at 96 px, shown small: `Icon` defaults to `px(11)`.

## Drawing style

- Outline / line icons: no filled shapes. Even `play`, `stop`, `cube-01`
  and `film-02` are outlines. `pause`, `plus`, `x`, `check` and
  `menu-01` are bare strokes.
- Uniform stroke, about 8 px of 96 (measured on `plus`, `pause`,
  `menu-01`, `info-circle`). Diagonals such as `x` measure about 8.5
  perpendicular.
- Round caps and round joins. Corners are rounded, including on `play`,
  `stop`, `save-01`, `check-square` and `trash-01`.
- Monochrome, single layer: no fills, gradients, shadows or duotone.
- Geometric and simple. Interior detail is kept to a minimum, e.g.
  `speedometer-04` has a dial, needle and ticks, and `cube-outline` is
  a dashed cube with corner arrows.
- Optical sizing is not uniform. The glyph bounding box is padded
  within the canvas by shape:
  - Full-bleed, about 4 px padding: circles and frames (`info-circle`,
    `placeholder`, `film-02`, `speedometer-04`, `folder`, `monitor-01`,
    `eye`).
  - About 8 px padding: most objects (`file-04`, `save-01`,
    `search-lg`, `trash-01`, `cube-*`).
  - About 16 px padding: simple marks (`plus`, `arrow-up`, `skip-*`).
  - About 24 px padding: `x`, which is the smallest.

## Naming

- kebab-case, with an optional two-digit variant suffix (`file-04`,
  `settings-04`, `home-05`, `menu-01`, `film-02`). This matches the
  naming of Untitled UI's icon set, and the drawing style matches its
  line icons. That provenance is inferred, not recorded in the repo.
- `cube-outline` is a plain name and probably custom.
- Directional pairs are mirrored exactly and share a pixel count
  (`reverse-left` / `reverse-right`, `skip-back` / `skip-forward`,
  `fast-backward` / `fast-forward`).

## Usage

- One asset per concept. Direction and state come from `Icon.rotation`
  rather than extra assets: `chevron-up` is rotated per fold state, and
  there are no separate down/left/right chevrons.
- Icons are visually inert: `Pickable { should_block_lower: false,
  is_hoverable: false }`, so the parent widget gets the pointer events.
- Hover animates `color` towards `hover_color`, if one is set.
- Reserve a transparent slot rather than omitting it when a tab has no
  icon (`placeholder.png`, a plain circle outline).
- Many icons in the folder have no const yet (`eye`, `check`,
  `save-01`, `home-05`, `x`, and so on). They are a stock of
  same-style spares.

## Binding an icon to a field

An icon can be bound to a reflected field with the `FieldIcon`
attribute (`moxie_ui::field_icon`), on a field or on the type itself:

```rust
#[derive(Component, Reflect)]
#[reflect(Component, @FieldIcon(icons::TRANSLATE))]
struct Velocity { ... }
```

For a type that can't carry it, `app.register_field_icon(
field!(<Transform>::translation), icons::TRANSLATE)` does the same
(`field!` is `motiongfx`'s `field_path::field!`), and wins over an
attribute on the same field. `field!(<Transform>)` binds the type itself.

`field_icon` looks up the field's own icon first, then each parent's up
to the type itself; none found means no icon. Timeline actions show
the result left of their label, shrinking and fading out as the bar
narrows.
