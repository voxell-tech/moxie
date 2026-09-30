use bevy_color::Color;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_text::{
    FontSize, LineBreak, TextColor, TextFont, TextLayout,
};
use bevy_ui::widget::Text;
use motiongfx_interp::interpolation::Interpolation;

use crate::prop::Prop;
use crate::tokens::{TextTokens, Tone};
use crate::transition::BevyMarker;
use crate::{Bevy, Leaf, Styled};

/// A run of text.
pub struct Label {
    pub text: Prop<String>,
    pub size: Prop<f32>,
    pub tone: Prop<Tone>,
    pub wrap: Prop<bool>,
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
}

impl Styled for Label {
    fn unset() -> Self {
        Self {
            text: Prop::Unset,
            size: Prop::Unset,
            tone: Prop::Unset,
            wrap: Prop::Unset,
        }
    }

    fn over(self, below: Self) -> Self {
        Self {
            text: self.text.or(below.text),
            size: self.size.or(below.size),
            tone: self.tone.or(below.tone),
            wrap: self.wrap.or(below.wrap),
        }
    }
}

/// A [`Label`]'s props at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelSnapshot {
    pub text: String,
    pub size: f32,
    pub color: Color,
    pub wrap: bool,
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
        }
    }
}

impl<T: TextTokens> Leaf<Bevy, T> for Label {
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
        ));
    }

    fn is_live(&self) -> bool {
        self.text.is_bound()
            || self.size.is_bound()
            || self.tone.is_bound()
            || self.wrap.is_bound()
    }

    fn changed(&mut self, world: &World) -> bool {
        self.text.changed(world)
            | self.size.changed(world)
            | self.tone.changed(world)
            | self.wrap.changed(world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_and_color_blend_while_text_and_wrap_snap() {
        let from = LabelSnapshot {
            text: "a".into(),
            size: 10.0,
            color: Color::BLACK,
            wrap: true,
        };
        let to = LabelSnapshot {
            text: "b".into(),
            size: 20.0,
            color: Color::WHITE,
            wrap: false,
        };

        let mid =
            <LabelSnapshot as Interpolation<BevyMarker>>::interp(
                &from, &to, 0.5,
            );

        assert_eq!(mid.text, "b");
        assert_eq!(mid.size, 15.0);
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
