//! Folding something away.
//!
//! [`Foldable`] is [`Fold`](crate::gaps::Fold) with moxie's chevron icon, rail
//! and sizes. Its state is an [`Open`](bevy_fynix::views::Open)
//! on its root node, or on an entity the caller keeps (see
//! [`Foldable::state_on`]), so a row rebuilt around it can keep its
//! state somewhere that survives.
//!
//! The body is built each time the fold opens and dropped when it
//! shuts, so a fold over something expensive (a filesystem read, say)
//! never pays for what nobody has looked at.

use bevy::asset::AssetServer;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy_fynix::{Bevy, Cx, View};

pub use crate::gaps::{Chevron, FoldOn as FoldsOn, RAIL_WIDTH};
use crate::gaps::{Fold, fold, rail};
use crate::icons;
use crate::theme::EditorTheme;

/// The chevron's rotation, clockwise from the asset's resting
/// up-pointing orientation. Right when shut, down when open.
pub const CHEVRON_SHUT: f32 = 90.0;
pub const CHEVRON_OPEN: f32 = 180.0;

/// The rail and indent a [`Foldable`]'s body sits under, for other
/// nested-but-unfoldable content to share.
pub fn indent<B>(body: B) -> impl View<Bevy, EditorTheme>
where
    B: View<Bevy, EditorTheme> + 'static,
{
    bevy_fynix::AnyView::new(
        move |cx: &mut Cx<'_, Bevy, EditorTheme>| {
            let theme = cx.theme();
            let view = rail(
                theme.space.fold_toggle / 2.0,
                theme.space.fold_indent,
                body,
            )
            .color(theme.palette.base[2]);
            cx.build(view)
        },
    )
}

/// A header that folds away the body under it.
///
/// `header` is called once with the [`Chevron`], so a section and a
/// tree row can look nothing alike and still fold the same way. This
/// owns the click that toggles, the chevron that turns, the body that
/// goes, and the rail marking how deep that body sits. `body` is
/// called each time the fold opens.
pub struct Foldable<H, B, F = fn(&mut World, bool)>(Fold<H, B, F>);

impl<H, B> Foldable<H, B> {
    /// A fold of `header` over `body`, open and folded by its header
    /// (see [`FoldsOn`]).
    pub fn new(header: H, body: B) -> Self {
        let chevron = Chevron::new(
            Default::default(),
            CHEVRON_SHUT,
            CHEVRON_OPEN,
        )
        .size(8.0);
        Self(fold(chevron, header, body))
    }
}

impl<H, B, F> Foldable<H, B, F> {
    /// What a click has to land on to fold: the header itself, or a
    /// chevron beside it that leaves the header free to mean
    /// something else, like selecting the row.
    pub fn folds_on(self, on: FoldsOn) -> Self {
        Self(self.0.on(on))
    }

    /// Whether there is anything to fold. A header with nothing under
    /// it neither turns nor toggles, and has no chevron of its own.
    pub fn enabled(self, enabled: bool) -> Self {
        Self(self.0.enabled(enabled))
    }

    /// Whether it starts open, as last left by the caller. Ignored
    /// under [`state_on`](Self::state_on).
    pub fn open(self, open: bool) -> Self {
        Self(self.0.open(open))
    }

    /// Keeps the state as `Open` on `entity`, which anything may
    /// change, instead of on the fold's own node.
    pub fn state_on(self, entity: Entity) -> Self {
        Self(self.0.state_on(entity))
    }

    /// Mirrors each click's new state into the caller's own store.
    /// A component on the entity a row stands for cleans itself up
    /// when the entity does.
    pub fn on_toggle<G>(self, on_toggle: G) -> Foldable<H, B, G>
    where
        G: Fn(&mut World, bool) + Send + Sync + 'static,
    {
        Foldable(self.0.on_toggle(on_toggle))
    }
}

impl<H, HV, B, BV, F> View<Bevy, EditorTheme> for Foldable<H, B, F>
where
    H: FnOnce(Chevron) -> HV,
    HV: View<Bevy, EditorTheme>,
    B: Fn() -> BV + Send + Sync + 'static,
    BV: View<Bevy, EditorTheme> + 'static,
    F: Fn(&mut World, bool) + Send + Sync + 'static,
{
    fn build(self, cx: &mut Cx<'_, Bevy, EditorTheme>) -> Entity {
        let image =
            cx.world.resource::<AssetServer>().load(icons::CHEVRON);
        let theme = cx.theme();
        let fold = self
            .0
            .image(image)
            .layout(theme.space.fold_toggle, theme.space.fold_indent)
            .rail_color(theme.palette.base[2]);
        cx.build(fold)
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicUsize, Ordering};
    use core::time::Duration;

    use bevy::app::TaskPoolPlugin;
    use bevy::asset::AssetPlugin;
    use bevy::image::Image;
    use bevy::math::Rot2;
    use bevy::prelude::*;
    use bevy::time::{TimePlugin, TimeUpdateStrategy};
    use bevy::ui::widget::ImageNode;
    use bevy::ui_widgets::{Activate, Button as ButtonBehavior};
    use bevy_fynix::mount;
    use bevy_fynix::views::{
        BehaviorExt as _, Open, button, label, row,
    };

    use super::*;
    use crate::MoxieUiPlugin;

    #[derive(Component)]
    struct Header;

    #[derive(Component)]
    struct Body;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            TimePlugin,
            AssetPlugin::default(),
            MoxieUiPlugin,
        ))
        .init_asset::<Image>()
        .insert_resource(
            TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(50),
            ),
        );
        app.update();
        app
    }

    fn fold_of(
        app: &mut App,
        on: FoldsOn,
        open: bool,
    ) -> (Entity, Entity) {
        let root = mount::<EditorTheme>(
            app.world_mut(),
            Foldable::new(
                |chevron: Chevron| {
                    button(row((chevron.icon(), label("head"))))
                        .tagged(Header)
                },
                || label("body").tagged(Body),
            )
            .folds_on(on)
            .open(open),
        );
        app.update();
        (root, header(app))
    }

    fn header(app: &mut App) -> Entity {
        app.world_mut()
            .query_filtered::<Entity, With<Header>>()
            .single(app.world())
            .unwrap()
    }

    fn bodies(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<Body>>()
            .iter(app.world())
            .count()
    }

    /// The nodes drawing a chevron, one per icon.
    fn chevrons(app: &mut App) -> Vec<Entity> {
        app.world_mut()
            .query_filtered::<Entity, With<ImageNode>>()
            .iter(app.world())
            .collect()
    }

    fn click(app: &mut App, entity: Entity) {
        app.world_mut().trigger(Activate { entity });
        app.update();
    }

    /// Runs the interact transition out.
    fn settle(app: &mut App) {
        for _ in 0..20 {
            app.update();
        }
    }

    fn rotation(app: &App, icon: Entity) -> Rot2 {
        app.world().get::<UiTransform>(icon).unwrap().rotation
    }

    fn shown(app: &App, root: Entity) -> bool {
        app.world().get::<Open>(root).is_some()
    }

    #[test]
    fn it_starts_open_or_shut_as_asked() {
        let mut open = app();
        let (root, _) = fold_of(&mut open, FoldsOn::Header, true);
        assert!(shown(&open, root));
        assert_eq!(bodies(&mut open), 1);

        let mut shut = app();
        let (root, _) = fold_of(&mut shut, FoldsOn::Header, false);
        assert!(!shown(&shut, root));
        assert_eq!(bodies(&mut shut), 0);
    }

    #[test]
    fn toggling_shows_and_hides_the_body_with_the_same_header() {
        let mut app = app();
        let (root, header) =
            fold_of(&mut app, FoldsOn::Header, false);

        click(&mut app, header);
        assert_eq!(bodies(&mut app), 1);
        assert_eq!(self::header(&mut app), header);

        click(&mut app, header);
        assert_eq!(bodies(&mut app), 0);
        assert!(!shown(&app, root));
        assert_eq!(self::header(&mut app), header);
    }

    #[test]
    fn a_shut_body_is_built_when_it_opens_and_each_time_after() {
        static BUILT: AtomicUsize = AtomicUsize::new(0);

        let mut app = app();
        let root = mount::<EditorTheme>(
            app.world_mut(),
            Foldable::new(
                |chevron: Chevron| {
                    button(chevron.icon()).tagged(Header)
                },
                || {
                    BUILT.fetch_add(1, Ordering::SeqCst);
                    label("body").tagged(Body)
                },
            )
            .open(false),
        );
        app.update();
        assert_eq!(BUILT.load(Ordering::SeqCst), 0);

        let header = header(&mut app);
        click(&mut app, header);
        assert_eq!(BUILT.load(Ordering::SeqCst), 1);
        click(&mut app, header);
        click(&mut app, header);
        assert_eq!(BUILT.load(Ordering::SeqCst), 2);
        assert!(shown(&app, root));
    }

    #[test]
    fn the_chevron_button_folds_when_it_is_the_one_asked() {
        let mut app = app();
        let (root, header) =
            fold_of(&mut app, FoldsOn::Chevron, true);

        // The header is not wired to fold.
        click(&mut app, header);
        assert!(shown(&app, root));

        // The chevron is the one image in the header's row and the
        // one the fold built beside it.
        let toggle = chevrons(&mut app)
            .into_iter()
            .map(|icon| app.world().get::<ChildOf>(icon).unwrap().0)
            .find(|parent| *parent != header)
            .and_then(|parent| {
                // The icon in the header sits under the header's
                // row; the toggle's own icon sits directly under its
                // button.
                app.world()
                    .get::<ButtonBehavior>(parent)
                    .map(|_| parent)
            });
        let toggle = toggle.expect("a chevron button");
        click(&mut app, toggle);
        assert!(!shown(&app, root));
        assert_eq!(bodies(&mut app), 0);
    }

    #[test]
    fn state_driven_from_outside_is_followed() {
        let mut app = app();
        let state = app.world_mut().spawn_empty().id();
        mount::<EditorTheme>(
            app.world_mut(),
            Foldable::new(
                |chevron: Chevron| {
                    button(row((chevron.icon(), label("head"))))
                        .tagged(Header)
                },
                || label("body").tagged(Body),
            )
            .state_on(state),
        );
        app.update();
        assert_eq!(bodies(&mut app), 0);

        app.world_mut().entity_mut(state).insert(Open);
        app.update();
        assert_eq!(bodies(&mut app), 1);

        // A click flips the outside state, not the root's.
        let header = header(&mut app);
        click(&mut app, header);
        assert!(app.world().get::<Open>(state).is_none());
        assert_eq!(bodies(&mut app), 0);
    }

    #[test]
    fn toggles_are_reported_to_the_caller() {
        #[derive(Resource, Default)]
        struct Last(Option<bool>);

        let mut app = app();
        app.init_resource::<Last>();
        mount::<EditorTheme>(
            app.world_mut(),
            Foldable::new(
                |chevron: Chevron| {
                    button(chevron.icon()).tagged(Header)
                },
                || label("body"),
            )
            .open(false)
            .on_toggle(|world, open| {
                world.resource_mut::<Last>().0 = Some(open);
            }),
        );
        app.update();
        let header = header(&mut app);
        click(&mut app, header);
        assert_eq!(app.world().resource::<Last>().0, Some(true));
        click(&mut app, header);
        assert_eq!(app.world().resource::<Last>().0, Some(false));
    }

    #[test]
    fn the_chevron_turns_with_the_state() {
        let mut app = app();
        let (_, header) = fold_of(&mut app, FoldsOn::Header, false);
        let icon = chevrons(&mut app)[0];
        assert_eq!(rotation(&app, icon), Rot2::degrees(CHEVRON_SHUT));

        click(&mut app, header);
        // It travels rather than jumping.
        assert_ne!(rotation(&app, icon), Rot2::degrees(CHEVRON_SHUT));
        assert_ne!(rotation(&app, icon), Rot2::degrees(CHEVRON_OPEN));
        settle(&mut app);
        assert_eq!(rotation(&app, icon), Rot2::degrees(CHEVRON_OPEN));

        click(&mut app, header);
        settle(&mut app);
        assert_eq!(rotation(&app, icon), Rot2::degrees(CHEVRON_SHUT));
    }

    #[test]
    fn a_fold_with_nothing_in_it_does_not_toggle() {
        let mut app = app();
        let root = mount::<EditorTheme>(
            app.world_mut(),
            Foldable::new(
                |chevron: Chevron| {
                    button(chevron.icon()).tagged(Header)
                },
                || label("body"),
            )
            .enabled(false)
            .open(false),
        );
        app.update();
        let header = header(&mut app);
        click(&mut app, header);

        assert!(!shown(&app, root));
        settle(&mut app);
        let icon = chevrons(&mut app)[0];
        assert_eq!(rotation(&app, icon), Rot2::degrees(CHEVRON_SHUT));
    }
}
