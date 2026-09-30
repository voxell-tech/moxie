//! The prototype's views in a window, one section per idea in
//! `docs/fynix_rewrite.md`: a theme implemented through token traits,
//! an app-wide set rule, bound labels, hover rules with transitions, a
//! scoped rule, a folding section, field rows, and a reduced-motion
//! switch.
//!
//! `cargo run -p bevy_fynix_proto --example gallery`

use core::time::Duration;

use bevy::DefaultPlugins;
use bevy::app::{App, Startup};
use bevy::camera::Camera2d;
use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::time::Time;
use bevy::ui::{AlignItems, FlexDirection, UiRect, percent, px};
use bevy_fynix_proto::tokens::{
    Curve, Motion, MotionTokens, SpacingTokens, SurfaceTokens,
    TextTokens, Tone,
};
use bevy_fynix_proto::views::{
    AnimatedField, BehaviorExt, HasAction, Label, LabelSnapshot,
    button, column, field_row, foldable, frame, label, row,
};
use bevy_fynix_proto::{
    AnyView, Bevy, Cx, FynixProtoPlugin, Hovered, ReducedMotion,
    StateExt, Theme, View, derived, mount,
};

/// What a view is built with, in this app.
type Build<'a> = Cx<'a, Bevy, Monokai>;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            FynixProtoPlugin::<Monokai>::default(),
        ))
        .insert_resource(Theme(Monokai))
        .insert_resource(Clicks(0))
        .add_systems(Startup, setup)
        .run();
}

/// The app's theme. Nothing in the views names it: they ask for
/// whatever token traits they need, and this implements them.
struct Monokai;

fn hex(rgb: u32) -> Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color::srgb_u8(r, g, b)
}

impl TextTokens for Monokai {
    fn tone(&self, tone: Tone) -> Color {
        match tone {
            Tone::Body => hex(0xFCFCFA),
            Tone::Dim => hex(0x939293),
            Tone::Accent => hex(0xFFD866),
        }
    }

    fn body_size(&self) -> f32 {
        14.0
    }

    fn small_size(&self) -> f32 {
        11.0
    }
}

impl SurfaceTokens for Monokai {
    fn fill(&self) -> Color {
        hex(0x403E41)
    }

    fn hover(&self) -> Color {
        hex(0x5B595C)
    }

    fn panel(&self) -> Color {
        hex(0x221F22)
    }
}

impl SpacingTokens for Monokai {
    fn gap(&self) -> f32 {
        8.0
    }

    fn row(&self) -> f32 {
        24.0
    }

    fn radius(&self) -> f32 {
        4.0
    }
}

impl MotionTokens for Monokai {
    fn motion(&self, motion: Motion) -> Curve {
        let millis = match motion {
            Motion::Interact => 180,
            Motion::Expand => 280,
        };
        Curve {
            duration: Duration::from_millis(millis),
            // Ease out: quick to answer, gentle to land.
            ease: |t| 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// How many times the counter's button was pressed.
#[derive(Resource)]
struct Clicks(u32);

fn setup(world: &mut World) {
    world.spawn(Camera2d);
    mount::<Monokai>(world, gallery());
}

fn gallery() -> AnyView<Bevy, Monokai> {
    AnyView::new(|cx: &mut Build| {
        // The app's preamble: every label is 13px unless its call
        // site or an inner scope says otherwise.
        cx.set::<Label>(|label, _| label.size(13.0));
        let panel = cx.theme().panel();
        cx.build(
            column((
                label("fynix prototype on Bevy").size(20.0),
                section("Bound values", bound_values()),
                section("Hover, with a transition", hover_list()),
                section("A scoped rule", scoped()),
                section("Folding", folding()),
                section("Field rows", fields()),
                section("Motion", motion_switch()),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .padding(UiRect::all(px(20.0)))
            .gap(18.0)
            .fill(panel),
        )
    })
}

/// A dim caption over its contents.
fn section(
    title: &'static str,
    body: impl View<Bevy, Monokai>,
) -> impl View<Bevy, Monokai> {
    column((label(title).size(11.0).tone(Tone::Dim), body)).gap(6.0)
}

/// Labels bound to the world, and buttons that change it.
fn bound_values() -> impl View<Bevy, Monokai> {
    let padding = UiRect::axes(px(10.0), px(4.0));
    column((
        row((
            label(derived(|world: &World| {
                format!(
                    "Clicked {} times",
                    world.resource::<Clicks>().0
                )
            })),
            button(label("+1")).padding(padding).on_activate(
                |world| world.resource_mut::<Clicks>().0 += 1,
            ),
            button(label("Reset")).padding(padding).on_activate(
                |world| world.resource_mut::<Clicks>().0 = 0,
            ),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
        label(derived(|world: &World| {
            format!(
                "{:.1}s since start",
                world.resource::<Time>().elapsed_secs()
            )
        }))
        .tone(Tone::Dim),
    ))
    .gap(6.0)
}

/// A label that turns accent under the pointer, over the theme's
/// curve.
fn line(text: &str) -> impl View<Bevy, Monokai> {
    label(text)
        .when::<Hovered>(
            |shown: &mut LabelSnapshot, theme: &Monokai| {
                shown.color = theme.tone(Tone::Accent);
                shown.size = 15.0;
            },
        )
        .transition(Motion::Interact)
}

fn hover_list() -> impl View<Bevy, Monokai> {
    column(
        ["cube.glb", "sphere.glb", "brick", "hello_world.mox"]
            .into_iter()
            .map(line)
            .collect::<Vec<_>>(),
    )
    .gap(4.0)
}

/// Rules set in a scope end with it, and an explicit value still
/// beats them.
fn scoped() -> AnyView<Bevy, Monokai> {
    AnyView::new(|cx: &mut Build| {
        let root = cx
            .build(frame().direction(FlexDirection::Column).gap(4.0));
        cx.under(root, |cx| {
            cx.scope(|cx| {
                cx.set::<Label>(|label, _| label.tone(Tone::Dim));
                cx.build(label(
                    "Inside the scope: dim, from one rule",
                ));
                cx.build(
                    label("An explicit tone still wins")
                        .tone(Tone::Accent),
                );
            });
            cx.build(label("After the scope: back to the preamble"));
        });
        root
    })
}

fn folding() -> impl View<Bevy, Monokai> {
    foldable(
        label("Assets"),
        column((
            label("meshes/cube.glb").tone(Tone::Dim),
            label("materials/brick").tone(Tone::Dim),
            label("scenes/hello_world.mox").tone(Tone::Dim),
        ))
        .gap(2.0)
        .padding(UiRect::left(px(24.0))),
    )
    .open(true)
}

/// A field row's label column stays on one line through a scoped rule.
/// The animatable ones turn accent while their node holds
/// [`HasAction`], which the button toggles.
fn fields() -> impl View<Bevy, Monokai> {
    column((
        field_row(
            label("translation").animatable::<Monokai>("translation"),
            label("0.0  0.0  0.0").tone(Tone::Dim),
        ),
        field_row(
            label("rotation").animatable::<Monokai>("rotation"),
            label("0.0  0.0  0.0").tone(Tone::Dim),
        ),
        field_row(
            label("a long field name, kept on one line"),
            label("plain, not animatable").tone(Tone::Dim),
        ),
        // A row of its own, so the column does not stretch it.
        row((button(label("Toggle keyframes"))
            .padding(UiRect::axes(px(10.0), px(4.0)))
            .on_activate(toggle_keyframes),)),
    ))
    .gap(4.0)
}

fn toggle_keyframes(world: &mut World) {
    let fields = world
        .query_filtered::<Entity, With<AnimatedField>>()
        .iter(world)
        .collect::<Vec<_>>();
    for field in fields {
        let mut field = world.entity_mut(field);
        if field.contains::<HasAction>() {
            field.remove::<HasAction>();
        } else {
            field.insert(HasAction);
        }
    }
}

fn motion_switch() -> impl View<Bevy, Monokai> {
    row((
        button(label(derived(|world: &World| {
            let on = world.resource::<ReducedMotion>().0;
            format!(
                "Reduced motion: {}",
                if on { "on" } else { "off" }
            )
        })))
        .padding(UiRect::axes(px(10.0), px(4.0)))
        .on_activate(|world| {
            let mut reduced = world.resource_mut::<ReducedMotion>();
            reduced.0 = !reduced.0;
        }),
        label("Hover the list above to compare").tone(Tone::Dim),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
}
