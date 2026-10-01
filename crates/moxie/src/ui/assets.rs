//! Bookmarked-folder asset browser.
//!
//! Bookmarks and the folders under them expand in place. A
//! recognized file can be dragged onto an inspector's `Handle<T>`
//! field; see `moxie_ui::asset`.

// STUB: ported in wave 3 step 2. Only the panel's view is a
// placeholder; what it will call is kept below.

use std::collections::BTreeSet;
use std::fs;
use std::ops::Bound;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_fynix::{AnyView, Bevy};
use moxie_ui::theme::EditorTheme;

use crate::ProjectBookmarks;

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
    // STUB: ported in wave 3 step 2
    super::stub_panel("Assets")
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
fn remove_bookmark(index: usize) -> impl FnOnce(&mut World) {
    move |world: &mut World| {
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
