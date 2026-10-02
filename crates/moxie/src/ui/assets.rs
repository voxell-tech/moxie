//! Bookmarked-folder asset browser.
//!
//! Bookmarks and the folders under them expand in place. A
//! recognized file can be dragged onto an inspector's `Handle<T>`
//! field; see `moxie_ui::asset`.

use std::collections::BTreeSet;
use std::fs;
use std::ops::Bound;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, button, column, frame, icon,
    icon_button, label, row, scroll, tint,
};
use bevy_fynix::{
    AnyView, Bevy, Cx, EntityCursor, ScopedExt as _, View,
    ViewExt as _, each,
};
use moxie_asset::AssetTypes;
use moxie_ui::asset::draggable;
use moxie_ui::fold::{Chevron, Foldable, FoldsOn};
use moxie_ui::gaps::{anchored, changing_under, inked};
use moxie_ui::theme::EditorTheme;

use crate::{ProjectBookmarks, ProjectPath};

/// Room below the last row for the button that floats over it.
const BUTTON_CLEARANCE: f32 = 34.0;

/// Which folders were left open, keyed by path, since nothing else
/// could hold this. A `BTreeSet`, since path components sort a
/// subtree into one run, so [`prune_fold_state`]/[`remove_bookmark`]
/// can drop it as a bounded range instead of walking every open
/// folder.
#[derive(Resource, Default)]
pub(super) struct AssetFoldState(BTreeSet<PathBuf>);

/// The assets panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        cx.build(
            column((
                anchored::<EditorTheme, _>(listing),
                add_button(),
            ))
            .width(percent(100.0))
            .height(percent(100.0)),
        )
    })
}

/// The bookmarks, one fold each, scrolling when they do not fit.
fn listing(anchor: Option<Entity>) -> impl View<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let pad = cx.theme().space.xl;
        cx.build(
            scroll((each(
                changing_under(anchor, bookmarks),
                |bookmark: &Bookmark| {
                    (bookmark.index, bookmark.path.clone())
                },
                |bookmark| {
                    fold_row(bookmark.path.clone(), bookmark.index)
                },
            )
            .within(
                frame().direction(FlexDirection::Column).gap(0.0),
            ),))
            .width(percent(100.0))
            .grow(1.0)
            .gap(0.0)
            .padding(UiRect::new(
                px(pad),
                px(pad),
                px(pad),
                px(BUTTON_CLEARANCE),
            ))
            .overflow(Overflow::scroll_y()),
        )
    })
}

/// One bookmark of the listing. `index` is its place in
/// [`ProjectBookmarks`], and `None` for the open project's own
/// folder, which is not in it and cannot be dropped.
#[derive(Clone, PartialEq)]
struct Bookmark {
    index: Option<usize>,
    path: PathBuf,
}

/// The project's own folder if there is one, then every bookmark.
fn bookmarks(world: &World) -> Vec<Bookmark> {
    let project = world
        .resource::<ProjectPath>()
        .0
        .as_deref()
        .and_then(Path::parent)
        .map(|path| Bookmark {
            index: None,
            path: path.to_path_buf(),
        });
    let listed = world
        .resource::<ProjectBookmarks>()
        .0
        .iter()
        .enumerate()
        .map(|(index, path)| Bookmark {
            index: Some(index),
            path: path.clone(),
        });
    project.into_iter().chain(listed).collect()
}

/// The button floated over the corner, so it stays put however far
/// the list is scrolled.
fn add_button() -> impl View<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        let plus = cx
            .world
            .resource::<AssetServer>()
            .load(crate::icons::PLUS);
        cx.build(
            button(icon(plus))
                .position(PositionType::Absolute)
                .inset(UiRect::new(auto(), px(pad), auto(), px(pad)))
                .rules(icon_button)
                .on_activate(add_bookmark),
        )
    })
}

/// Prompts for a folder and bookmarks it. Dropped silently if the
/// dialog was dismissed, or if it nests with a bookmark already
/// there in either direction, since the row it would open into and
/// the row already showing it would otherwise both list the same
/// files.
fn add_bookmark(world: &mut World) {
    let Some(folder) = rfd::FileDialog::new().pick_folder() else {
        return;
    };
    push_bookmark(world, folder);
}

/// Bookmarks `folder` unless it nests with a bookmark already there.
fn push_bookmark(world: &mut World, folder: PathBuf) {
    let mut bookmarks = world.resource_mut::<ProjectBookmarks>();
    let nests = bookmarks.0.iter().any(|existing| {
        folder.starts_with(existing) || existing.starts_with(&folder)
    });
    if nests {
        return;
    }
    bookmarks.0.push(folder);
}

/// Drops the bookmark at `index`, and every fold state key under the
/// folder it named. A subfolder's key would otherwise sit unread
/// forever, since nothing else names it once the bookmark is gone.
fn remove_bookmark(world: &mut World, index: usize) {
    let mut bookmarks = world.resource_mut::<ProjectBookmarks>();
    if index >= bookmarks.0.len() {
        return;
    }
    let removed = bookmarks.0.remove(index);

    let mut state = world.resource_mut::<AssetFoldState>();
    let gone = state
        .0
        .range(removed.clone()..)
        .take_while(|path| path.starts_with(&removed))
        .cloned()
        .collect::<Vec<_>>();
    for path in gone {
        state.0.remove(&path);
    }
}

/// A folder's row: its name, expanding its contents in place. A
/// bookmark (`removable`) also has a button to drop it, which takes
/// its own click, so the fold never hears it.
fn fold_row(
    path: PathBuf,
    removable: Option<usize>,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let enabled = has_entries(&path);
        let open =
            cx.world.resource::<AssetFoldState>().0.contains(&path);
        let trash = removable.map(|index| {
            let image = cx
                .world
                .resource::<AssetServer>()
                .load(crate::icons::TRASH);
            (index, image)
        });
        let name = display_name(&path);
        let body = path.clone();
        let toggled = path.clone();

        cx.build(
            Foldable::new(
                move |chevron: Chevron| header(chevron, name, trash),
                move || contents(body.clone()),
            )
            .folds_on(FoldsOn::Header)
            .enabled(enabled)
            .open(open)
            .on_toggle(move |world, open| {
                let mut state =
                    world.resource_mut::<AssetFoldState>();
                if open {
                    state.0.insert(toggled.clone());
                } else {
                    state.0.remove(&toggled);
                }
            }),
        )
    })
}

/// The clickable head of a [`fold_row`]. With `trash`, a delete
/// button for the bookmark at its index sits flush against the far
/// end.
fn header(
    chevron: Chevron,
    name: String,
    trash: Option<(usize, Handle<Image>)>,
) -> impl View<Bevy, EditorTheme> {
    let mut content =
        vec![chevron.icon().boxed(), label(name).wrap(false).boxed()];
    let removable = trash.is_some();
    if let Some((index, image)) = trash {
        // Eats whatever room icon and label leave.
        content.push(frame().grow(1.0).boxed());
        content.push(
            button(icon(image).size(10.0))
                .width(px(14.0))
                .height(px(14.0))
                .padding(UiRect::ZERO)
                .radius(2.0)
                .rules(tint)
                .on_activate(move |world| {
                    remove_bookmark(world, index);
                })
                .toned(Tone::Critical)
                .boxed(),
        );
    }

    let mut content = row(content).align(AlignItems::Center);
    if removable {
        content = content.width(percent(100.0));
    }
    let mut head = button(content)
        .padding(UiRect::vertical(px(3.0)))
        .justify(JustifyContent::FlexStart);
    if removable {
        head = head.width(percent(100.0));
    }
    head.rules(tint)
}

/// `path`'s direct entries, directories before files, read as the
/// view is built.
fn contents(path: PathBuf) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let Ok(entries) = fs::read_dir(&path) else {
            return cx.build(frame());
        };

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                dirs.push(entry_path);
            } else {
                files.push(entry_path);
            }
        }
        dirs.sort();
        files.sort();

        prune_fold_state(cx.world, &path, &dirs);

        let mut rows = Vec::new();
        for dir in dirs {
            rows.push(fold_row(dir, None));
        }
        for file in files {
            rows.push(file_row(cx, file));
        }
        cx.build(column(rows).gap(0.0).width(percent(100.0)))
    })
}

/// One file. A recognized asset type (see `AssetTypes`) can be
/// dragged onto an inspector's `Handle<T>` field, in the theme's
/// accent with a grab cursor to say so. Anything else shows dimmer,
/// to read as inert.
fn file_row(
    cx: &mut Cx<'_, Bevy, EditorTheme>,
    path: PathBuf,
) -> AnyView<Bevy, EditorTheme> {
    let name = display_name(&path);
    let kind = cx.world.resource::<AssetTypes>().kind_of(&path);
    let purple = cx.theme().palette.purple;

    let text = label(name.clone()).wrap(false);
    let text = match kind {
        Some(_) => inked(text, purple).boxed(),
        None => text.tone(Tone::Dim).boxed(),
    };
    let line = row((text,))
        .width(percent(100.0))
        .padding(UiRect::vertical(px(3.0)));

    match kind {
        Some(kind) => draggable(
            line.with(EntityCursor(SystemCursorIcon::Grab)),
            path,
            kind,
            name,
        )
        .boxed(),
        None => line.boxed(),
    }
}

/// Whether `path` has anything in it, without collecting it. A row
/// only needs to know there is something to fold.
fn has_entries(path: &Path) -> bool {
    fs::read_dir(path)
        .is_ok_and(|mut entries| entries.next().is_some())
}

/// Drops every open-state key under a direct child of `path` that
/// `current` no longer lists. Bounded to `path`'s own subtree, see
/// [`AssetFoldState`].
fn prune_fold_state(
    world: &mut World,
    path: &Path,
    current: &[PathBuf],
) {
    let mut state = world.resource_mut::<AssetFoldState>();

    // Only the stale roots themselves get cloned here. `filter`
    // runs on borrowed entries before `cloned` ever touches one, so
    // a descendant that isn't actually going anywhere is never
    // copied just to be looked at.
    let stale_roots = state
        .0
        .range((
            Bound::Excluded(path.to_path_buf()),
            Bound::Unbounded,
        ))
        .take_while(|open| open.starts_with(path))
        .filter(|open| {
            open.parent() == Some(path) && !current.contains(open)
        })
        .cloned()
        .collect::<Vec<_>>();

    // Each root's own subtree. Most of what `path` holds survives
    // untouched and is never cloned at all.
    for root in stale_roots {
        let gone = state
            .0
            .range(root.clone()..)
            .take_while(|open| open.starts_with(&root))
            .cloned()
            .collect::<Vec<_>>();
        for open in gone {
            state.0.remove(&open);
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use core::time::Duration;
    use std::any::TypeId;

    use bevy::app::TaskPoolPlugin;
    use bevy::asset::AssetPlugin;
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{DragStart, Pointer};
    use bevy::picking::pointer::{
        Location, PointerButton, PointerId,
    };
    use bevy::time::{TimePlugin, TimeUpdateStrategy};
    use bevy::ui::widget::Text;
    use bevy::ui_widgets::{Activate, Button as ButtonBehavior};
    use bevy_fynix::mount;
    use moxie_asset::AssetTypeAppExt as _;
    use moxie_ui::MoxieUiPlugin;
    use moxie_ui::asset::AssetDragging;

    use super::*;

    /// A directory under the system's temp one, gone when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "moxie_assets_{}_{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn dir(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            TimePlugin,
            AssetPlugin::default(),
            MoxieUiPlugin,
        ))
        .init_asset::<Image>()
        .init_resource::<UiScale>()
        .init_resource::<AssetFoldState>()
        .init_resource::<ProjectBookmarks>()
        .init_resource::<ProjectPath>()
        .insert_resource(
            TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(50),
            ),
        );
        app.asset_type::<Image>().extensions.push("png");
        app.update();
        app
    }

    /// An app with the panel mounted, over `bookmarks`.
    fn shown(bookmarks: Vec<PathBuf>) -> App {
        let mut app = app();
        app.world_mut().resource_mut::<ProjectBookmarks>().0 =
            bookmarks;
        mount::<EditorTheme>(app.world_mut(), panel());
        settle(&mut app);
        app
    }

    fn settle(app: &mut App) {
        for _ in 0..3 {
            app.update();
        }
    }

    /// The child index of `node` under each ancestor, root first, so
    /// that sorting by it is top-to-bottom order.
    fn place(world: &World, mut node: Entity) -> Vec<usize> {
        let mut at = Vec::new();
        while let Some(parent) = world.get::<ChildOf>(node) {
            let parent = parent.0;
            let index = world
                .get::<Children>(parent)
                .and_then(|kids| kids.iter().position(|k| k == node))
                .unwrap_or(0);
            at.push(index);
            node = parent;
        }
        at.reverse();
        at
    }

    /// Every text shown, top to bottom.
    fn texts(app: &mut App) -> Vec<String> {
        let world = app.world_mut();
        let mut found = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .map(|(entity, text)| {
                (place(world, entity), text.0.clone())
            })
            .collect::<Vec<_>>();
        found.sort();
        found.into_iter().map(|(_, text)| text).collect()
    }

    fn text_node(app: &mut App, shown: &str) -> Entity {
        let world = app.world_mut();
        world
            .query::<(Entity, &Text)>()
            .iter(world)
            .find(|(_, text)| text.0 == shown)
            .map(|(entity, _)| entity)
            .unwrap_or_else(|| panic!("no text {shown:?}"))
    }

    /// The nearest button above `node`.
    fn button_above(app: &App, mut node: Entity) -> Entity {
        loop {
            if app.world().get::<ButtonBehavior>(node).is_some() {
                return node;
            }
            node = app
                .world()
                .get::<ChildOf>(node)
                .expect("a button above")
                .0;
        }
    }

    /// Every button below `node`.
    fn buttons_below(app: &App, node: Entity) -> Vec<Entity> {
        let mut found = Vec::new();
        let mut pending = vec![node];
        while let Some(next) = pending.pop() {
            if let Some(children) = app.world().get::<Children>(next)
            {
                for child in children.iter() {
                    if app
                        .world()
                        .get::<ButtonBehavior>(child)
                        .is_some()
                    {
                        found.push(child);
                    }
                    pending.push(child);
                }
            }
        }
        found
    }

    fn click(app: &mut App, button: Entity) {
        app.world_mut().trigger(Activate { entity: button });
        settle(app);
    }

    /// Clicks the button the label `head` sits in.
    fn click_head(app: &mut App, head: Entity) {
        let button = button_above(app, head);
        click(app, button);
    }

    fn fold_state(app: &App) -> Vec<PathBuf> {
        app.world()
            .resource::<AssetFoldState>()
            .0
            .iter()
            .cloned()
            .collect()
    }

    fn open_set(paths: &[&Path]) -> AssetFoldState {
        AssetFoldState(
            paths.iter().map(|path| path.to_path_buf()).collect(),
        )
    }

    #[test]
    fn the_project_folder_comes_before_the_bookmarks() {
        let mut app = app();
        app.world_mut().resource_mut::<ProjectPath>().0 =
            Some(PathBuf::from("/p/scene.mox"));
        app.world_mut().resource_mut::<ProjectBookmarks>().0 =
            vec![PathBuf::from("/a"), PathBuf::from("/b")];

        let listed = bookmarks(app.world())
            .into_iter()
            .map(|bookmark| (bookmark.index, bookmark.path))
            .collect::<Vec<_>>();
        assert_eq!(
            listed,
            [
                (None, PathBuf::from("/p")),
                (Some(0), PathBuf::from("/a")),
                (Some(1), PathBuf::from("/b")),
            ]
        );
    }

    #[test]
    fn a_new_bookmark_nesting_with_one_is_dropped() {
        let mut app = app();
        app.world_mut().resource_mut::<ProjectBookmarks>().0 =
            vec![PathBuf::from("/a/b")];

        for folder in ["/a/b", "/a/b/c", "/a"] {
            push_bookmark(app.world_mut(), PathBuf::from(folder));
        }
        let held = &app.world().resource::<ProjectBookmarks>().0;
        assert_eq!(held, &[PathBuf::from("/a/b")]);

        push_bookmark(app.world_mut(), PathBuf::from("/a/other"));
        let held = &app.world().resource::<ProjectBookmarks>().0;
        assert_eq!(held.len(), 2);
    }

    #[test]
    fn removing_a_bookmark_forgets_the_folders_under_it() {
        let mut app = app();
        app.world_mut().resource_mut::<ProjectBookmarks>().0 =
            vec![PathBuf::from("/a"), PathBuf::from("/ab")];
        *app.world_mut().resource_mut::<AssetFoldState>() =
            open_set(&[
                Path::new("/a"),
                Path::new("/a/x/y"),
                Path::new("/ab"),
            ]);

        remove_bookmark(app.world_mut(), 0);

        let held = &app.world().resource::<ProjectBookmarks>().0;
        assert_eq!(held, &[PathBuf::from("/ab")]);
        assert_eq!(fold_state(&app), [PathBuf::from("/ab")]);
    }

    #[test]
    fn removing_past_the_end_changes_nothing() {
        let mut app = app();
        app.world_mut().resource_mut::<ProjectBookmarks>().0 =
            vec![PathBuf::from("/a")];
        remove_bookmark(app.world_mut(), 3);
        assert_eq!(
            app.world().resource::<ProjectBookmarks>().0.len(),
            1
        );
    }

    #[test]
    fn pruning_drops_only_the_subtrees_no_longer_listed() {
        let mut app = app();
        *app.world_mut().resource_mut::<AssetFoldState>() =
            open_set(&[
                Path::new("/r/gone"),
                Path::new("/r/gone/deep"),
                Path::new("/r/kept"),
                Path::new("/r/kept/deep"),
                Path::new("/other"),
            ]);

        prune_fold_state(
            app.world_mut(),
            Path::new("/r"),
            &[PathBuf::from("/r/kept")],
        );

        assert_eq!(
            fold_state(&app),
            [
                PathBuf::from("/other"),
                PathBuf::from("/r/kept"),
                PathBuf::from("/r/kept/deep"),
            ]
        );
    }

    #[test]
    fn a_folder_is_foldable_only_with_something_in_it() {
        let scratch = Scratch::new("entries");
        let empty = scratch.dir("empty");
        let full = scratch.dir("full");
        fs::write(full.join("a.txt"), b"x").unwrap();

        assert!(!has_entries(&empty));
        assert!(has_entries(&full));
        assert!(!has_entries(&scratch.0.join("missing")));
    }

    #[test]
    fn a_bookmark_shows_its_name_shut() {
        let scratch = Scratch::new("shut");
        let folder = scratch.dir("work");
        fs::write(folder.join("a.txt"), b"x").unwrap();

        let mut app = shown(vec![folder]);
        assert_eq!(texts(&mut app), ["work"]);
    }

    #[test]
    fn opening_a_bookmark_lists_folders_then_files_and_remembers() {
        let scratch = Scratch::new("open");
        let folder = scratch.dir("work");
        fs::write(folder.join("b.txt"), b"x").unwrap();
        fs::write(folder.join("a.txt"), b"x").unwrap();
        fs::create_dir_all(folder.join("zdir")).unwrap();
        fs::create_dir_all(folder.join("adir")).unwrap();
        fs::write(folder.join("adir").join("in.txt"), b"x").unwrap();

        let mut app = shown(vec![folder.clone()]);
        let head = text_node(&mut app, "work");
        click_head(&mut app, head);

        assert_eq!(
            texts(&mut app),
            ["work", "adir", "zdir", "a.txt", "b.txt"]
        );
        assert_eq!(fold_state(&app), std::slice::from_ref(&folder));

        click_head(&mut app, head);
        assert_eq!(texts(&mut app), ["work"]);
        assert!(fold_state(&app).is_empty());
    }

    #[test]
    fn a_folder_left_open_opens_again_when_rebuilt() {
        let scratch = Scratch::new("reopen");
        let folder = scratch.dir("work");
        fs::write(folder.join("a.txt"), b"x").unwrap();
        let mut app = app();
        *app.world_mut().resource_mut::<AssetFoldState>() =
            open_set(&[&folder]);
        app.world_mut().resource_mut::<ProjectBookmarks>().0 =
            vec![folder];

        mount::<EditorTheme>(app.world_mut(), panel());
        settle(&mut app);

        assert_eq!(texts(&mut app), ["work", "a.txt"]);
    }

    #[test]
    fn a_subfolder_opens_in_place() {
        let scratch = Scratch::new("sub");
        let folder = scratch.dir("work");
        let sub = folder.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("deep.txt"), b"x").unwrap();

        let mut app = shown(vec![folder.clone()]);
        let head = text_node(&mut app, "work");
        click_head(&mut app, head);
        let head = text_node(&mut app, "sub");
        click_head(&mut app, head);

        assert_eq!(texts(&mut app), ["work", "sub", "deep.txt"]);
        assert_eq!(fold_state(&app), [folder, sub]);
    }

    #[test]
    fn an_empty_folder_does_not_fold() {
        let scratch = Scratch::new("hollow");
        let folder = scratch.dir("work");
        let mut app = shown(vec![folder]);

        let head = text_node(&mut app, "work");
        click_head(&mut app, head);

        assert_eq!(texts(&mut app), ["work"]);
        assert!(fold_state(&app).is_empty());
    }

    #[test]
    fn the_project_folder_has_no_delete_button() {
        let scratch = Scratch::new("project");
        let folder = scratch.dir("work");
        fs::write(folder.join("scene.mox"), b"x").unwrap();
        let mut app = app();
        app.world_mut().resource_mut::<ProjectPath>().0 =
            Some(folder.join("scene.mox"));
        mount::<EditorTheme>(app.world_mut(), panel());
        settle(&mut app);

        let head = text_node(&mut app, "work");
        let head = button_above(&app, head);
        assert!(buttons_below(&app, head).is_empty());
    }

    #[test]
    fn a_bookmark_is_dropped_by_its_own_button() {
        let scratch = Scratch::new("drop");
        let keep = scratch.dir("keep");
        let gone = scratch.dir("gone");
        fs::write(keep.join("a.txt"), b"x").unwrap();
        fs::write(gone.join("b.txt"), b"x").unwrap();

        let mut app = shown(vec![keep.clone(), gone]);
        let head = text_node(&mut app, "gone");
        let head = button_above(&app, head);
        let [trash] = buttons_below(&app, head)[..] else {
            panic!("one delete button");
        };
        click(&mut app, trash);

        let held = &app.world().resource::<ProjectBookmarks>().0;
        assert_eq!(held, &[keep]);
        assert_eq!(texts(&mut app), ["keep"]);
    }

    #[test]
    fn deleting_does_not_fold_the_row() {
        let scratch = Scratch::new("nofold");
        let folder = scratch.dir("work");
        fs::write(folder.join("a.txt"), b"x").unwrap();
        let second = scratch.dir("second");
        fs::write(second.join("b.txt"), b"x").unwrap();

        let mut app = shown(vec![folder, second]);
        let head = text_node(&mut app, "work");
        let head = button_above(&app, head);
        let trash = buttons_below(&app, head)[0];
        click(&mut app, trash);

        assert!(fold_state(&app).is_empty());
        assert_eq!(texts(&mut app), ["second"]);
    }

    #[test]
    fn the_listing_follows_the_bookmarks() {
        let scratch = Scratch::new("follow");
        let first = scratch.dir("first");
        let second = scratch.dir("second");
        let mut app = shown(vec![first]);
        assert_eq!(texts(&mut app), ["first"]);

        app.world_mut()
            .resource_mut::<ProjectBookmarks>()
            .0
            .push(second);
        settle(&mut app);

        assert_eq!(texts(&mut app), ["first", "second"]);
    }

    #[test]
    fn only_a_recognized_file_can_be_picked_up() {
        let scratch = Scratch::new("kinds");
        let folder = scratch.dir("work");
        fs::write(folder.join("a.png"), b"x").unwrap();
        fs::write(folder.join("b.txt"), b"x").unwrap();
        let mut app = shown(vec![folder.clone()]);
        let head = text_node(&mut app, "work");
        click_head(&mut app, head);

        let png = text_node(&mut app, "a.png");
        let txt = text_node(&mut app, "b.txt");
        let row_of = |app: &App, node: Entity| {
            app.world().get::<ChildOf>(node).unwrap().0
        };
        let png_row = row_of(&app, png);
        let txt_row = row_of(&app, txt);
        assert!(app.world().get::<EntityCursor>(png_row).is_some());
        assert!(app.world().get::<EntityCursor>(txt_row).is_none());

        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(10.0, 10.0),
        };
        let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
        let start = DragStart {
            button: PointerButton::Primary,
            hit,
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location.clone(),
            start.clone(),
            txt_row,
        ));
        app.update();
        assert!(
            app.world().resource::<AssetDragging>().path.is_none()
        );

        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            start,
            png_row,
        ));
        app.update();
        let dragging = app.world().resource::<AssetDragging>();
        assert_eq!(dragging.path, Some(folder.join("a.png")));
        assert_eq!(dragging.kind, Some(TypeId::of::<Image>()));
    }
}
