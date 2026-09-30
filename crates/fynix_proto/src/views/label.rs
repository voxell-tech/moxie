use bevy::prelude::*;

use crate::prop::Prop;
use crate::tokens::{TextTokens, Tone};
use crate::view::{Leaf, Styled};

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

impl<T: TextTokens> Leaf<T> for Label {
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
                ..default()
            },
            TextColor(snapshot.color),
            TextLayout {
                linebreak,
                ..default()
            },
        ));
    }

    fn is_live(&self) -> bool {
        self.text.is_bound()
            || self.size.is_bound()
            || self.tone.is_bound()
            || self.wrap.is_bound()
    }
}
