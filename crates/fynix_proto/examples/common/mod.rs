//! The theme, the screen and the run harness the screen examples
//! share. `screen!` takes the name of a wrapping macro that is applied
//! at every composite: `wrap_none` leaves the types nested, and
//! `wrap_boxed` erases them.

use core::time::Duration;

pub use bevy::prelude::*;
#[allow(unused_imports)]
pub use fynix_proto::ViewExt;
pub use fynix_proto::modifier::ModifierExt;
pub use fynix_proto::tokens::{
    Curve, Motion, MotionTokens, SpacingTokens, SurfaceTokens,
    TextTokens, Tone,
};
pub use fynix_proto::views::{
    LabelSnapshot, button, column, icon, label, row,
};
pub use fynix_proto::{
    Bevy, FynixProtoPlugin, Hovered, StateExt, Stateful, Theme, View,
    mount,
};

/// The theme the screens are built under.
pub struct Editor;

impl TextTokens for Editor {
    fn tone(&self, tone: Tone) -> Color {
        match tone {
            Tone::Body => Color::WHITE,
            Tone::Dim => Color::srgb(0.6, 0.6, 0.6),
            Tone::Accent => Color::srgb(1.0, 0.5, 0.0),
        }
    }

    fn body_size(&self) -> f32 {
        14.0
    }

    fn small_size(&self) -> f32 {
        11.0
    }
}

impl SurfaceTokens for Editor {
    fn fill(&self) -> Color {
        Color::srgb(0.2, 0.2, 0.2)
    }

    fn hover(&self) -> Color {
        Color::srgb(0.3, 0.3, 0.3)
    }

    fn panel(&self) -> Color {
        Color::BLACK
    }
}

impl SpacingTokens for Editor {
    fn gap(&self) -> f32 {
        6.0
    }

    fn row(&self) -> f32 {
        20.0
    }

    fn radius(&self) -> f32 {
        3.0
    }
}

impl MotionTokens for Editor {
    fn motion(&self, _: Motion) -> Curve {
        Curve {
            duration: Duration::from_millis(120),
            ease: |t| t,
        }
    }
}

/// A label that turns accent on hover, over the theme's curve.
pub fn line(
    text: String,
) -> Stateful<fynix_proto::views::Label, Editor> {
    label(text)
        .when::<Hovered>(
            |snapshot: &mut LabelSnapshot, theme: &Editor| {
                snapshot.color = theme.tone(Tone::Accent);
            },
        )
        .transition(Motion::Interact)
}

pub fn glyph() -> fynix_proto::views::Icon {
    icon(Handle::<Image>::default()).size(14.0)
}

#[allow(unused_macros)]
macro_rules! wrap_none {
    ($view:expr) => {
        $view
    };
}

#[allow(unused_macros)]
macro_rules! wrap_boxed {
    ($view:expr) => {
        <_ as ViewExt<Bevy, Editor>>::boxed($view)
    };
}

/// The four modifier stacks a row of shapes can wear, each a
/// different type.
macro_rules! mod0 {
    ($view:expr) => {
        $view
    };
}

macro_rules! mod1 {
    ($view:expr) => {
        ModifierExt::padding($view, UiRect::all(px(2.0)))
    };
}

macro_rules! mod2 {
    ($view:expr) => {
        ModifierExt::width(mod1!($view), px(200.0))
    };
}

macro_rules! mod3 {
    ($view:expr) => {
        ModifierExt::grow(mod2!($view), 1.0)
    };
}

/// Six siblings of six different shapes, all dressed with `$modify`.
macro_rules! shapes {
    ($wrap:ident, $modify:ident, $i:expr) => {
        (
            $modify!($wrap!(button($wrap!(row((
                glyph(),
                line(format!("A {}", $i))
            )))))),
            $modify!($wrap!(button($wrap!(row((
                glyph(),
                line(format!("B {}", $i)),
                line(format!("b {}", $i))
            )))))),
            $modify!($wrap!(row((
                line(format!("C {}", $i)),
                glyph(),
                line(format!("c {}", $i))
            )))),
            $modify!($wrap!(button($wrap!(column((
                line(format!("D {}", $i)),
                $wrap!(row((glyph(), line(format!("d {}", $i)))))
            )))))),
            $modify!($wrap!(button($wrap!(row((
                glyph(),
                glyph(),
                line(format!("E {}", $i))
            )))))),
            $modify!($wrap!(column((
                line(format!("F {}", $i)),
                line(format!("f {}", $i)),
                line(format!("g {}", $i))
            )))),
        )
    };
}

/// Columns nested one level per literal, ending in `$leaf`.
macro_rules! nest {
    ($wrap:ident; $leaf:expr;) => {
        $leaf
    };
    ($wrap:ident; $leaf:expr; $first:literal $($rest:literal)*) => {
        $wrap!(column((
            line($first.to_string()),
            nest!($wrap; $leaf; $($rest)*)
        ))
        .gap(2.0))
    };
}

/// Defines `screen::screen()`: four panels, each with a title, a
/// toolbar of five shapes, six sibling shapes, ten command rows,
/// a column nested five deep and a footer.
macro_rules! screen {
    ($wrap:ident) => {
        mod screen {
            use crate::common::*;

            fn commands(panel: usize) -> impl View<Bevy, Editor> {
                $wrap!(column(
                    (0..10)
                        .map(|i| {
                            $wrap!(button($wrap!(row((
                                glyph(),
                                line(format!("Command {panel}.{i}"))
                            )))))
                        })
                        .collect::<Vec<_>>()
                ))
            }

            fn toolbar() -> impl View<Bevy, Editor> {
                $wrap!(row((
                    $wrap!(button(glyph())),
                    $wrap!(button(line("Run".to_string()))),
                    $wrap!(button($wrap!(row((
                        glyph(),
                        line("Stop".to_string())
                    ))))),
                    glyph(),
                    line("Ready now".to_string()),
                )))
            }

            fn footer() -> impl View<Bevy, Editor> {
                $wrap!(row((
                    line("Items".to_string()),
                    glyph(),
                    line("Saved".to_string()),
                )))
            }

            fn nested() -> impl View<Bevy, Editor> {
                nest!(
                    $wrap;
                    $wrap!(button($wrap!(row((
                        glyph(),
                        line("Deep".to_string())
                    )))));
                    "One" "Two" "Three" "Four" "Five"
                )
            }

            macro_rules! panel {
                ($modify:ident, $n:expr, $title:literal) => {
                    $wrap!(column((
                        line($title.to_string()),
                        toolbar(),
                        $wrap!(column(shapes!($wrap, $modify, $n))),
                        commands($n),
                        nested(),
                        footer(),
                    )))
                };
            }

            pub fn screen() -> impl View<Bevy, Editor> {
                column((
                    panel!(mod0, 0, "Scene"),
                    panel!(mod1, 1, "Assets"),
                    panel!(mod2, 2, "Inspector"),
                    panel!(mod3, 3, "Console"),
                ))
            }
        }
    };
}

fn depth(world: &World, node: Entity) -> usize {
    1 + world
        .get::<Children>(node)
        .map(|children| {
            children
                .iter()
                .map(|child| depth(world, child))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// Mounts `view` in a headless app, runs one update and prints what
/// was built.
pub fn run<V: View<Bevy, Editor>>(name: &str, view: V) {
    println!(
        "{name}: top-level type name is {} chars",
        core::any::type_name::<V>().len()
    );

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        FynixProtoPlugin::<Editor>::default(),
    ))
    .insert_resource(Theme(Editor));
    let root = mount::<Editor>(app.world_mut(), view);
    app.update();

    let nodes =
        app.world_mut().query::<&Node>().iter(app.world()).count();
    println!(
        "{name}: {nodes} nodes, depth {}",
        depth(app.world(), root)
    );
}
