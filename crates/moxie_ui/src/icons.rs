//! Icon asset paths moxie_ui's own widgets reach for, relative to
//! the shared `editor/assets` folder (see `AssetPlugin::file_path` in
//! the consuming app). An app's *own* icons (panel tabs, playback,
//! ...) belong in the app's crate instead. This is only for icons
//! the dock/inspector engine itself draws.

/// A transparent icon, keeping the slot of a dock tab with no icon.
pub const PLACEHOLDER: &str = "icons/general/placeholder.png";

/// A plus sign, for a button that adds something.
pub const PLUS: &str = "icons/general/plus.png";

/// A dropdown's chevron. Points up.
pub const CHEVRON: &str = "icons/arrows/chevron-up.png";
/// A shut fold's chevron.
pub const CHEVRON_RIGHT: &str = "icons/arrows/chevron-right.png";
/// An open fold's chevron.
pub const CHEVRON_DOWN: &str = "icons/arrows/chevron-down.png";

/// A `Handle<T>` field in the inspector.
pub const ASSET: &str = "icons/files/file-04.png";

/// Closes a window.
pub const CLOSE: &str = "icons/general/x.png";

/// A destructive menu row, like "Delete".
pub const TRASH: &str = "icons/general/trash-01.png";
