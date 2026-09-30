use bevy::asset::Handle;
use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::ui::widget::ImageNode;
use bevy::ui::{Node, px};

use motiongfx_interp::interpolation::{InterpFn, Interpolation};

use crate::prop::Prop;
use crate::state::own_when;
use crate::tokens::{TextTokens, Tone};
use crate::transition::BevyMarker;
use crate::{Bevy, Element, Styled};

/// A square image tinted by a text tone.
pub struct Icon {
    pub image: Prop<Handle<Image>>,
    pub size: Prop<f32>,
    pub tone: Prop<Tone>,
}

pub fn icon(image: impl Into<Prop<Handle<Image>>>) -> Icon {
    Icon {
        image: image.into(),
        ..Icon::unset()
    }
}

impl Icon {
    pub fn image(
        mut self,
        image: impl Into<Prop<Handle<Image>>>,
    ) -> Self {
        self.image = image.into();
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
}

fynix_proto::styled!(Icon { image, size, tone });

own_when!(Icon);

/// An [`Icon`]'s props at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct IconSnapshot {
    pub image: Handle<Image>,
    pub size: f32,
    pub color: Color,
}

/// The size and tint blend, and the image takes the target.
impl Interpolation<BevyMarker> for IconSnapshot {
    fn interp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            image: to.image.clone(),
            size: <f32 as Interpolation<()>>::interp(
                &from.size, &to.size, t,
            ),
            color: <Color as Interpolation<BevyMarker>>::interp(
                &from.color,
                &to.color,
                t,
            ),
        }
    }
}

impl<T: TextTokens> Element<Bevy, T> for Icon {
    type Snapshot = IconSnapshot;

    fn prepare(world: &mut World, node: Entity) {
        world.entity_mut(node).insert(ImageNode::default());
    }

    fn snapshot(&self, world: &World, theme: &T) -> IconSnapshot {
        let tone = self.tone.get(world).unwrap_or_default();
        IconSnapshot {
            image: self.image.get(world).unwrap_or_default(),
            size: self.size.get(world).unwrap_or(theme.body_size()),
            color: theme.tone(tone),
        }
    }

    fn write(
        snapshot: &IconSnapshot,
        world: &mut World,
        node: Entity,
    ) {
        let mut entity = world.entity_mut(node);
        if let Some(mut ui) = entity.get_mut::<Node>() {
            ui.width = px(snapshot.size);
            ui.height = px(snapshot.size);
        }
        entity.insert(
            ImageNode::new(snapshot.image.clone())
                .with_color(snapshot.color),
        );
    }

    fn is_live(&self) -> bool {
        self.image.is_bound()
            || self.size.is_bound()
            || self.tone.is_bound()
    }

    fn changed(&mut self, world: &World) -> bool {
        self.image.changed(world)
            | self.size.changed(world)
            | self.tone.changed(world)
    }

    fn interp() -> Option<InterpFn<IconSnapshot>> {
        Some(<IconSnapshot as Interpolation<BevyMarker>>::interp)
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::hierarchy::Children;
    use bevy::ecs::relationship::RelationshipTarget;
    use bevy::time::TimePlugin;
    use bevy::ui::Val;

    use super::*;
    use crate::{AnyView, FynixProtoPlugin, Theme, mount};

    struct Plain;

    impl TextTokens for Plain {
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

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixProtoPlugin::<Plain>::default(),
        ))
        .insert_resource(Theme(Plain));
        app
    }

    fn image_node(app: &App, node: Entity) -> &ImageNode {
        app.world().get::<ImageNode>(node).expect("an icon")
    }

    #[test]
    fn unset_props_fall_back_to_the_theme() {
        let mut app = app();
        let node =
            mount::<Plain>(app.world_mut(), icon(Handle::default()));

        assert_eq!(image_node(&app, node).color, Color::WHITE);
        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.width, Val::Px(14.0));
        assert_eq!(ui.height, Val::Px(14.0));
    }

    #[test]
    fn props_are_written_and_the_tone_picks_the_colour() {
        let mut app = app();
        let handle = Handle::<Image>::default();
        let node = mount::<Plain>(
            app.world_mut(),
            icon(handle.clone()).size(20.0).tone(Tone::Accent),
        );

        assert_eq!(image_node(&app, node).image, handle);
        assert_eq!(
            image_node(&app, node).color,
            Color::srgb(1.0, 0.5, 0.0)
        );
        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.width, Val::Px(20.0));
    }

    #[test]
    fn a_set_rule_beats_the_theme_and_the_call_site_beats_it() {
        let mut app = app();
        let root = mount::<Plain>(
            app.world_mut(),
            AnyView::<Bevy, Plain>::new(|cx| {
                let root = cx.spawn();
                cx.under(root, |cx| {
                    cx.set::<Icon>(|i, _| {
                        i.size(30.0).tone(Tone::Dim)
                    });
                    cx.build(icon(Handle::default()));
                    cx.build(icon(Handle::default()).size(8.0));
                });
                root
            }),
        );
        let kids = app
            .world()
            .get::<Children>(root)
            .unwrap()
            .iter()
            .collect::<Vec<_>>();

        let width =
            |node| app.world().get::<Node>(node).unwrap().width;
        assert_eq!(width(kids[0]), Val::Px(30.0));
        assert_eq!(width(kids[1]), Val::Px(8.0));
        assert_eq!(
            image_node(&app, kids[1]).color,
            Color::srgb(0.5, 0.5, 0.5)
        );
    }
}
