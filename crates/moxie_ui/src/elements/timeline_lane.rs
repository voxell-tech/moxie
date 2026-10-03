use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{Node, UiRect, Val, percent, px};
use bevy_fynix::views::frame;
use bevy_fynix::{AnyView, Bevy, View, ViewSeq};

use super::placement::Placement;
use crate::theme::EditorTheme;

/// A track's lane: an absolutely placed strip as wide as its parent,
/// `height` tall and `top` from the parent's top, with a hairline
/// under it.
pub fn timeline_lane(
    top: Val,
    height: f32,
) -> impl View<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let theme = cx.theme();
        let width = theme.space.hairline;
        let lane = cx.build(
            Placement::new(
                Val::ZERO,
                top,
                percent(100.0),
                px(height),
            )
            .apply(frame())
            .border_color(theme.color.hairline),
        );
        set_bottom_border(cx.world, lane, width);
        lane
    })
}

/// A border of `width` on the bottom edge only.
fn set_bottom_border(world: &mut World, lane: Entity, width: f32) {
    if let Some(mut ui) = world.get_mut::<Node>(lane) {
        ui.border = UiRect::bottom(px(width));
    }
}

/// The time range a track covers: an invisible, absolutely placed
/// frame holding `children`, which sit in it by percent.
pub fn timeline_span<C>(
    placement: Placement,
    children: C,
) -> impl View<Bevy, EditorTheme>
where
    C: ViewSeq<Bevy, EditorTheme> + 'static,
{
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let span = cx.build(placement.apply(frame()));
        cx.under(span, |cx| children.build_each(cx));
        span
    })
}
