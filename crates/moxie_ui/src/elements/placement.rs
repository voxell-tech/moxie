use bevy::ui::{PositionType, UiRect, Val};
use bevy_fynix::Prop;
use bevy_fynix::views::Frame;

use bevy::ecs::world::World;

/// Where a timeline box sits and how big it is, in pixels from its
/// parent's top left corner.
pub struct Placement {
    left: Prop<Val>,
    top: Val,
    width: Prop<Val>,
    height: Val,
}

impl Placement {
    pub fn new(
        left: impl Into<Prop<Val>>,
        top: Val,
        width: impl Into<Prop<Val>>,
        height: Val,
    ) -> Self {
        Self {
            left: left.into(),
            top,
            width: width.into(),
            height,
        }
    }

    /// The width in pixels, if it is one.
    pub(super) fn width_px(&self, world: &World) -> Option<f32> {
        match self.width.get(world)? {
            Val::Px(width) => Some(width),
            _ => None,
        }
    }

    /// `frame`, absolutely placed here.
    pub(super) fn apply(self, frame: Frame) -> Frame {
        let top = self.top;
        frame
            .position(PositionType::Absolute)
            .inset(self.left.map(move |left| UiRect {
                left,
                top,
                right: Val::Auto,
                bottom: Val::Auto,
            }))
            .width(self.width)
            .height(self.height)
    }
}
