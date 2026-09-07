# Moxie

[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/voxell-tech/moxie#license)
[![CI](https://github.com/voxell-tech/moxie/workflows/CI/badge.svg)](https://github.com/voxell-tech/moxie/actions)
[![Discord](https://img.shields.io/discord/442334985471655946.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.gg/Mhnyp6VYEQ)

**Moxie** is a `bevy_ui` based editor for the
[MotionGfx](https://github.com/voxell-tech/motiongfx) framework:
timeline, hierarchy, inspector, dockable panels, and a scene document
backed by `motiongfx_scene`.

It renders a docked timeline panel for the first `Timeline` it finds:
scrub by pressing or dragging the track, toggle play/pause with the
button or spacebar, and scroll the track with a resizable name column.

## Workspace

| Crate | Description |
| ----- | ----------- |
| [`moxie`](https://github.com/voxell-tech/moxie/tree/main/crates/moxie) | The editor app. |
| [`moxie_ui`](https://github.com/voxell-tech/moxie/tree/main/crates/moxie_ui) | Reusable `bevy_ui` widgets, docking, and theming, built on `fynix`. |
| [`moxie_asset`](https://github.com/voxell-tech/moxie/tree/main/crates/moxie_asset) | Asset-kind registry, absolute asset source, and the `.mat` loader. |

## Building

Moxie depends on `bevy_motiongfx` / `motiongfx_scene` from the `moxie`
branch of `voxell-tech/motiongfx`, which is not yet on `main`. Until
that lands, `motiongfx_interp` is pinned through a `[patch]` to a local
checkout, so `motiongfx` must be cloned beside this repo on the `moxie`
branch:

```text
~/src/moxie      <- this repo
~/src/motiongfx  <- voxell-tech/motiongfx, on branch `moxie`
```

```sh
cargo run -p moxie
```

## Join the community!

You can join us on the [Voxell discord server](https://discord.gg/Mhnyp6VYEQ).

## License

`moxie` is dual-licensed under either:

- MIT License ([LICENSE-MIT](/LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](/LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

This means you can select the license you prefer!
This dual-licensing approach is the de-facto standard in the Rust ecosystem and there are [very good reasons](https://github.com/bevyengine/bevy/issues/2373) to include both.
