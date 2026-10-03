//! The views an app builds with: `bevy_fynix`'s, plus moxie's own.

pub use bevy_fynix::views::{
    AnimatedField, Axis, BehaviorExt, Button, Checkbox, ContextMenu,
    ContextMenuExt, Divider, Dropdown, Extra, FieldRow, Foldable,
    Frame, FrameProps, HasAction, Icon, IconProps, Label, LabelProps,
    MENU_Z, MenuItem, NumberField, OnActivate, Open, Segmented,
    Stack, TOOLTIP_Z, Tagged, TextField, TextInput, TextInputProps,
    Toned, Tooltip, TooltipExt, TooltipTiming, button, checkbox,
    column, divider, dropdown, field_row, foldable, frame, ghost,
    icon, label, menu_bar, menu_item, menu_surface, number_field,
    overlay, row, scroll, segment, segmented, text_field, tint,
};

mod inspector;
mod placement;
mod playhead;
mod time_label;
mod time_tick;
mod timeline_action;
mod timeline_block;
mod timeline_gap;
mod timeline_lane;
mod timeline_link;
mod timeline_track;

pub use inspector::{
    asset_card, component_inspector, component_inspector_of,
    display_name, entity_inspector, resource_inspector,
    resource_inspector_of, root_inspector,
};
pub use placement::Placement;
pub use playhead::playhead_line;
pub use time_label::time_label;
pub use time_tick::time_tick;
pub use timeline_action::{
    ACTION_ICON_SIZE, ActionClip, ActionGlyph, fit_action_icons,
    icon_fit, timeline_action,
};
pub use timeline_block::{Selected, timeline_block};
pub use timeline_gap::timeline_gap;
pub use timeline_lane::{timeline_lane, timeline_span};
pub use timeline_link::timeline_link;
pub use timeline_track::timeline_track;
