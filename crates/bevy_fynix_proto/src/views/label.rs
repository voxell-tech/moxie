use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::text::{
    FontSize, LineBreak, TextColor, TextFont, TextLayout,
};
use bevy::ui::UiTransform;
use bevy::ui::widget::Text;
use motiongfx_interp::interpolation::{InterpFn, Interpolation};

use crate::prop::Prop;
use crate::state::own_when;
use crate::tokens::{TextTokens, Tone};
use crate::transition::BevyMarker;
use crate::{Bevy, Element, Styled};

/// A run of text.
pub struct Label {
    pub text: Prop<String>,
    pub size: Prop<f32>,
    pub tone: Prop<Tone>,
    pub wrap: Prop<bool>,
    /// The factor the node is scaled by around its centre after
    /// layout, 1.0 when unset.
    pub scale: Prop<f32>,
}

pub fn label(text: impl Into<Prop<String>>) -> Label {
    Label {
        text: text.into(),
        ..Label::unset()
    }
}

impl Label {
    pub fn text(mut self, text: impl Into<Prop<String>>) -> Self {
        self.text = text.into();
        self
    }

    pub fn size(mut self, size: impl Into<Prop<f32>>) -> Self {
        self.size = size.into();
        self
    }

    pub fn tone(mut self, tone: impl Into<Prop<Tone>>) -> Self {
        self.tone = tone.into();
        self
    }

    pub fn wrap(mut self, wrap: impl Into<Prop<bool>>) -> Self {
        self.wrap = wrap.into();
        self
    }

    pub fn scale(mut self, scale: impl Into<Prop<f32>>) -> Self {
        self.scale = scale.into();
        self
    }
}

fynix_proto::styled!(Label {
    text,
    size,
    tone,
    wrap,
    scale
});

own_when!(Label);

/// A [`Label`]'s props at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelSnapshot {
    pub text: String,
    pub size: f32,
    pub color: Color,
    pub wrap: bool,
    pub scale: f32,
}

impl Interpolation<BevyMarker> for LabelSnapshot {
    fn interp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            text: to.text.clone(),
            size: <f32 as Interpolation<()>>::interp(
                &from.size, &to.size, t,
            ),
            color: <Color as Interpolation<BevyMarker>>::interp(
                &from.color,
                &to.color,
                t,
            ),
            wrap: to.wrap,
            scale: <f32 as Interpolation<()>>::interp(
                &from.scale,
                &to.scale,
                t,
            ),
        }
    }
}

impl<T: TextTokens> Element<Bevy, T> for Label {
    type Snapshot = LabelSnapshot;

    fn prepare(world: &mut World, node: Entity) {
        world.entity_mut(node).insert((
            Text::default(),
            TextFont::default(),
            TextColor::default(),
            TextLayout::default(),
        ));
    }

    fn snapshot(&self, world: &World, theme: &T) -> LabelSnapshot {
        let tone = self.tone.get(world).unwrap_or_default();
        LabelSnapshot {
            text: self.text.get(world).unwrap_or_default(),
            size: self.size.get(world).unwrap_or(theme.body_size()),
            color: theme.tone(tone),
            wrap: self.wrap.get(world).unwrap_or(true),
            scale: self.scale.get(world).unwrap_or(1.0),
        }
    }

    fn write(
        snapshot: &LabelSnapshot,
        world: &mut World,
        node: Entity,
    ) {
        let linebreak = if snapshot.wrap {
            LineBreak::WordBoundary
        } else {
            LineBreak::NoWrap
        };
        world.entity_mut(node).insert((
            Text::new(snapshot.text.clone()),
            TextFont {
                font_size: FontSize::Px(snapshot.size),
                ..Default::default()
            },
            TextColor(snapshot.color),
            TextLayout {
                linebreak,
                ..Default::default()
            },
            UiTransform::from_scale(Vec2::splat(snapshot.scale)),
        ));
    }

    fn is_live(&self) -> bool {
        self.text.is_bound()
            || self.size.is_bound()
            || self.tone.is_bound()
            || self.wrap.is_bound()
            || self.scale.is_bound()
    }

    fn changed(&mut self, world: &World) -> bool {
        self.text.changed(world)
            | self.size.changed(world)
            | self.tone.changed(world)
            | self.wrap.changed(world)
            | self.scale.changed(world)
    }

    fn interp() -> Option<InterpFn<LabelSnapshot>> {
        Some(<LabelSnapshot as Interpolation<BevyMarker>>::interp)
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use bevy::app::App;
    use bevy::time::{TimePlugin, TimeUpdateStrategy};

    use super::*;
    use crate::tokens::{Curve, Motion, MotionTokens};
    use crate::transition::ReducedMotion;
    use crate::{FynixProtoPlugin, Hovered, ScopedExt, Theme, mount};

    struct Plain;

    impl TextTokens for Plain {
        fn tone(&self, _: Tone) -> Color {
            Color::WHITE
        }

        fn body_size(&self) -> f32 {
            14.0
        }

        fn small_size(&self) -> f32 {
            11.0
        }
    }

    impl MotionTokens for Plain {
        fn motion(&self, _: Motion) -> Curve {
            Curve {
                duration: Duration::from_millis(100),
                ease: |t| t,
            }
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixProtoPlugin::<Plain>::default(),
        ))
        .insert_resource(Theme(Plain))
        .insert_resource(
            TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(50),
            ),
        );
        // The first update only starts the clock.
        app.update();
        app
    }

    fn scale(app: &App, node: Entity) -> Vec2 {
        app.world().get::<UiTransform>(node).unwrap().scale
    }

    fn grow(label: Label, _: &Plain) -> Label {
        label.scale(2.0)
    }

    #[test]
    fn an_unset_scale_is_the_identity_transform() {
        let mut app = app();
        let node = mount::<Plain>(app.world_mut(), label("x"));

        assert_eq!(
            app.world().get::<UiTransform>(node),
            Some(&UiTransform::IDENTITY)
        );
    }

    #[test]
    fn a_scale_is_written_to_both_axes_and_nothing_else() {
        let mut app = app();
        let node =
            mount::<Plain>(app.world_mut(), label("x").scale(1.2));

        assert_eq!(
            app.world().get::<UiTransform>(node),
            Some(&UiTransform::from_scale(Vec2::splat(1.2)))
        );
    }

    #[test]
    fn a_hover_rule_moves_the_scale_over_the_curve_not_the_size() {
        let mut app = app();
        let node = mount::<Plain>(
            app.world_mut(),
            label("x")
                .when::<Hovered, _>(grow)
                .transition(Motion::Interact),
        );

        app.world_mut().entity_mut(node).insert(Hovered);
        app.update();
        assert_eq!(
            scale(&app, node),
            Vec2::splat(1.5),
            "50ms of 100ms"
        );
        let font = app.world().get::<TextFont>(node).unwrap();
        assert_eq!(font.font_size, FontSize::Px(14.0));

        app.update();
        assert_eq!(scale(&app, node), Vec2::splat(2.0));
    }

    #[test]
    fn reduced_motion_snaps_the_scale() {
        let mut app = app();
        app.insert_resource(ReducedMotion(true));
        let node = mount::<Plain>(
            app.world_mut(),
            label("x")
                .when::<Hovered, _>(grow)
                .transition(Motion::Interact),
        );

        app.world_mut().entity_mut(node).insert(Hovered);
        app.update();

        assert_eq!(scale(&app, node), Vec2::splat(2.0));
    }

    #[test]
    fn size_color_and_scale_blend_while_text_and_wrap_snap() {
        let from = LabelSnapshot {
            text: "a".into(),
            size: 10.0,
            color: Color::BLACK,
            wrap: true,
            scale: 1.0,
        };
        let to = LabelSnapshot {
            text: "b".into(),
            size: 20.0,
            color: Color::WHITE,
            wrap: false,
            scale: 2.0,
        };

        let mid =
            <LabelSnapshot as Interpolation<BevyMarker>>::interp(
                &from, &to, 0.5,
            );

        assert_eq!(mid.text, "b");
        assert_eq!(mid.size, 15.0);
        assert_eq!(mid.scale, 1.5);
        assert_eq!(
            mid.color,
            <Color as Interpolation<BevyMarker>>::interp(
                &from.color,
                &to.color,
                0.5
            )
        );
        assert!(!mid.wrap);
    }
}
