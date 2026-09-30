//! A headless app per test, and two unrelated themes to build under.

use bevy_app::App;
use bevy_color::Color;
use bevy_ecs::entity::Entity;
use bevy_ecs::hierarchy::Children;
use bevy_ecs::relationship::RelationshipTarget;
use bevy_ecs::resource::Resource;
use bevy_text::{
    FontSize, LineBreak, TextColor, TextFont, TextLayout,
};
use bevy_time::TimePlugin;
use bevy_ui::widget::Text;

use crate::mounted::Mounts;
use crate::tokens::{TextTokens, Tone};
use crate::views::label;
use crate::{AnyView, Bevy, FynixProtoPlugin, Theme, derived, mount};

/// One app's theme.
struct Warm;

impl TextTokens for Warm {
    fn tone(&self, tone: Tone) -> Color {
        match tone {
            Tone::Body => Color::WHITE,
            Tone::Dim => Color::srgb(0.5, 0.5, 0.5),
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

/// Another app's, stored nothing like the first.
struct Cold {
    sizes: [f32; 2],
}

impl TextTokens for Cold {
    fn tone(&self, _: Tone) -> Color {
        Color::srgb(0.0, 0.5, 1.0)
    }

    fn body_size(&self) -> f32 {
        self.sizes[0]
    }

    fn small_size(&self) -> f32 {
        self.sizes[1]
    }
}

fn app<T: Send + Sync + 'static>(theme: T) -> App {
    let mut app = App::new();
    app.add_plugins((TimePlugin, FynixProtoPlugin::<T>::default()))
        .insert_resource(Theme(theme));
    app
}

fn size(app: &App, node: Entity) -> FontSize {
    app.world()
        .get::<TextFont>(node)
        .expect("a label")
        .font_size
}

fn text(app: &App, node: Entity) -> String {
    app.world().get::<Text>(node).expect("a label").0.clone()
}

fn color(app: &App, node: Entity) -> Color {
    app.world().get::<TextColor>(node).expect("a label").0
}

/// The nodes built directly under `root`, in order.
fn children(app: &App, root: Entity) -> Vec<Entity> {
    app.world()
        .get::<Children>(root)
        .map(|children| children.iter().collect())
        .unwrap_or_default()
}

#[test]
fn an_unset_prop_falls_back_to_the_theme() {
    let mut app = app(Warm);
    let node = mount::<Warm>(app.world_mut(), label("Save"));

    assert_eq!(text(&app, node), "Save");
    assert_eq!(size(&app, node), FontSize::Px(14.0));
    assert_eq!(color(&app, node), Color::WHITE);
}

#[test]
fn the_same_view_works_under_two_unrelated_themes() {
    let mut warm = app(Warm);
    let mut cold = app(Cold {
        sizes: [20.0, 16.0],
    });
    let in_warm = mount::<Warm>(warm.world_mut(), label("x"));
    let in_cold = mount::<Cold>(cold.world_mut(), label("x"));

    assert_eq!(size(&warm, in_warm), FontSize::Px(14.0));
    assert_eq!(size(&cold, in_cold), FontSize::Px(20.0));
}

#[test]
fn a_set_rule_fills_what_the_call_site_left_unset() {
    let mut app = app(Warm);
    let root = mount::<Warm>(
        app.world_mut(),
        AnyView::<Bevy, Warm>::new(|cx| {
            let root = cx.spawn();
            cx.under(root, |cx| {
                cx.set::<crate::views::Label>(|l, _| {
                    l.size(20.0).tone(Tone::Dim)
                });
                cx.build(label("ruled"));
                cx.build(label("explicit").size(9.0));
            });
            root
        }),
    );
    let [ruled, explicit] = children(&app, root)[..] else {
        panic!("two labels");
    };

    assert_eq!(size(&app, ruled), FontSize::Px(20.0));
    assert_eq!(
        size(&app, explicit),
        FontSize::Px(9.0),
        "call site wins"
    );
    assert_eq!(color(&app, explicit), Color::srgb(0.5, 0.5, 0.5));
}

#[test]
fn an_inner_scope_wins_and_ends_with_its_scope() {
    let mut app = app(Warm);
    let root = mount::<Warm>(
        app.world_mut(),
        AnyView::<Bevy, Warm>::new(|cx| {
            let root = cx.spawn();
            cx.under(root, |cx| {
                cx.set::<crate::views::Label>(|l, _| l.size(20.0));
                cx.scope(|cx| {
                    cx.set::<crate::views::Label>(|l, _| {
                        l.size(30.0)
                    });
                    cx.build(label("inner"));
                });
                cx.build(label("after"));
            });
            root
        }),
    );
    let [inner, after] = children(&app, root)[..] else {
        panic!("two labels");
    };

    assert_eq!(size(&app, inner), FontSize::Px(30.0));
    assert_eq!(size(&app, after), FontSize::Px(20.0));
}

#[test]
fn a_rule_can_read_the_theme() {
    let mut app = app(Warm);
    let root = mount::<Warm>(
        app.world_mut(),
        AnyView::<Bevy, Warm>::new(|cx| {
            let root = cx.spawn();
            cx.under(root, |cx| {
                cx.set::<crate::views::Label>(|l, theme: &Warm| {
                    l.size(theme.small_size())
                });
                cx.build(label("small"));
            });
            root
        }),
    );

    assert_eq!(
        size(&app, children(&app, root)[0]),
        FontSize::Px(11.0)
    );
}

#[test]
fn a_show_rule_wins_over_the_call_site() {
    let mut app = app(Warm);
    let root = mount::<Warm>(
        app.world_mut(),
        AnyView::<Bevy, Warm>::new(|cx| {
            let root = cx.spawn();
            cx.under(root, |cx| {
                cx.show::<crate::views::Label>(|l, _| l.wrap(false));
                cx.build(label("x").wrap(true));
            });
            root
        }),
    );
    let node = children(&app, root)[0];

    let layout =
        app.world().get::<TextLayout>(node).expect("a label");
    assert_eq!(layout.linebreak, LineBreak::NoWrap);
}

#[derive(Resource)]
struct Count(u32);

#[test]
fn a_bound_prop_follows_the_world() {
    let mut app = app(Warm);
    app.insert_resource(Count(1));
    let bound = mount::<Warm>(
        app.world_mut(),
        label(derived(|world| {
            world.resource::<Count>().0.to_string()
        })),
    );
    let fixed = mount::<Warm>(app.world_mut(), label("fixed"));
    assert_eq!(text(&app, bound), "1");

    app.world_mut().resource_mut::<Count>().0 = 2;
    app.update();

    assert_eq!(text(&app, bound), "2");
    assert_eq!(text(&app, fixed), "fixed");
    assert_eq!(
        app.world().resource::<Mounts<Warm>>().len(),
        1,
        "only what can change stays mounted"
    );
}

#[test]
fn a_despawned_view_is_dropped() {
    let mut app = app(Warm);
    app.insert_resource(Count(1));
    let node = mount::<Warm>(
        app.world_mut(),
        label(derived(|world| {
            world.resource::<Count>().0.to_string()
        })),
    );
    app.world_mut().despawn(node);
    app.update();

    assert!(app.world().resource::<Mounts<Warm>>().is_empty());
}
