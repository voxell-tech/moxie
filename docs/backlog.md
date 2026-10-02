# Backlog

## Action editing follow-ups

What is left of the action editing round. Creating an action by
dragging a field onto the timeline, naming, collapsing, moving and
resizing are done.

- [ ] Graduating a draft once its subject and field are both picked:
      `op` defaults to `AnimOp::To`, and `value` to the field's live
      value in a new pool slot. Open question: whether it fires the
      moment both are set, or waits for a confirm. Drag-to-create may
      make this path unneeded.
- [ ] Demote the actions of a deleted entity to drafts as it is
      deleted, from the `On<Remove, EntityUid>` observer in
      `bevy_motiongfx/scene/id.rs`. Today a failing compile demotes
      them, but only at the next recompile.
- [ ] Partly demote an action whose component was removed: keep the
      subject, clear the field, op and value.
- [ ] Ask before a deletion that would orphan live actions. There is no
      undo.
- [ ] Edit the stage: a toggle on an inspector field row pinning its
      live value as the stage seed, and a matching row in the action
      panel for the earliest action on a field.
- [ ] A dashed ghost clip while a field is dragged over the timeline.
- [ ] Set a new node's `delay` from where it is dropped inside `All` or
      `Flow`.
- [ ] Move the drop internals shared by `reorder` and `create` into
      their own `timeline/drop.rs`.
- [ ] Incremental names for new entities ("Cube", "Cube 1", ...).
- [ ] An Operation row once `AnimOp` has a second variant.

## Open a `.mox` by double-clicking it

Needs moxie shipped as an installed app: the OS only routes a document
to something it has a registration for. Windows and Linux hand the path
over as `argv[1]`, so reading it is `env::args_os().nth(1)` either way;
registration is a `HKCU\Software\Classes` key on one and a `.desktop`
plus a shared-mime-info XML on the other.

macOS is the awkward one. Finder never passes the path in `argv` - it
sends a `kAEOpenDocuments` Apple Event, and only to a real `.app`
bundle. Winit's macOS docs say it "guarantees that it will not register
an application delegate" and show an `application:openURLs:` example,
but that is wrong for 0.30: it registers `WinitApplicationDelegate` in
`EventLoop::new`, and its run loop asserts every iteration that the
delegate is still that type, so setting our own panics on the first
turn. The way in is `NSAppleEventManager`'s
`setEventHandler:andSelector:forEventClass:andEventID:`, which leaves
`NSApp.delegate` alone - registered after `EventLoop::new` but before
the app runs, or the launching document is missed. `objc2-app-kit` and
`objc2-foundation` are already in the tree, but on two major versions
at once: winit is on the 0.5/0.2 generation, and types do not cross.
`setEventHandler:` has no binding, so it needs a raw `msg_send!`.

Two things break outside `cargo run` whatever we do, since both resolve
a path that only exists on the build machine: `AssetPlugin::file_path`
is `"../assets"`, relative to a working directory a launched app does
not have, and `project.rs`'s file dialog starts at
`env!("CARGO_MANIFEST_DIR")`.

Dragging a `.mox` onto the running window is a separate path that
already works - bevy maps it to `FileDragAndDrop::DroppedFile` - and
would be worth wiring up on its own.

- [ ] Resolve assets and the dialog's starting folder against the
      running executable rather than the build machine.
- [ ] Handle `FileDragAndDrop::DroppedFile`.
- [ ] macOS: bundle, its `Info.plist` document-type declarations, and
      the Apple Event handler.
- [ ] Windows and Linux: register the type, and decide whether that is
      an installer's job or something moxie does for itself on first
      run.

## "Open" dialog never remembers the last-used location

Reported: "Open" should start wherever a project was last opened or
saved from, but doesn't - not a case of another picker's state leaking
in, there's simply no last-used-location memory at all today.

`project.rs`'s `ask_for_path` unconditionally calls
`.set_directory(&scenes)`, `scenes` being the fixed
`CARGO_MANIFEST_DIR/../assets/scenes` - every Open and every Save
starts there, every time, regardless of what was picked last (this
session or a previous one). `ProjectPath` (already updated by both
`save_scene` and `load_scene` on success) is the obvious source of
truth for "last used" - `ask_for_path` just doesn't read it, and isn't
even passed `world` today to be able to.

- [ ] Thread `world: &World` (or just the resolved `Option<PathBuf>`)
      into `ask_for_path`, and seed `.set_directory(...)` from
      `ProjectPath.0`'s parent when set, falling back to `scenes` only
      when nothing has been opened or saved yet.
- [ ] Decide whether Open and Save should track the same "last
      location", or diverge (e.g. Save defaulting to the current
      project's own folder, Open to wherever was last opened).
- [ ] Persisting this across app restarts (not just within one running
      session) would need somewhere durable to keep it -
      `EditorSettings` is the existing precedent for that.

## Assigning and treating assets in the inspector

`Handle<T>` fields (`MeshMaterial3d<StandardMaterial>`, `Mesh3d`, and
anything else asset-bearing) fall through the reflect-tree inspector
today as an opaque struct - there's no UI to assign one at all.
`project.rs`'s `.mox` save/load already round-trips a `Handle<T>` as a
path string, through bevy's own `world_serialization` (`WorldDeserializer`
+ `LoadFromPath`) - that's inherited plumbing, not something moxie
built, and `Mesh3d`/`MeshMaterial3d<StandardMaterial>` are already
allowlisted in `subject_components()` to go through it.
`moxie_asset`'s `StdMaterialAssetLoader` (the `.mat` loader) is the
existing precedent for a project-authored asset: a `StandardMaterial`
reflected to RON, loaded through the normal `AssetServer`/`Handle`
path like anything else - it just has no caller today besides a
one-off codegen example.

Modeled on Unity/Unreal rather than Blender/After Effects: one asset
root per project (a folder next to, or named by, the `.mox`), external
files imported into it on assignment rather than referenced in place
from anywhere on disk. This sidesteps the path-root fragility already
flagged above (`AssetPlugin::file_path` hardcoded to `"../assets"`,
broken outside a dev build) without needing a GUID/redirector system -
still plain path references, so a rename done outside the editor can
still break one, same limitation Blender/AE's relative-path model has.

Small project-authored assets (a material) get a second option:
`mox://` as a scheme on the same `Handle<T>` path, backed by
`bevy_asset::io::memory::{Dir, MemoryAssetReader}` (already built into
bevy, not hand-rolled) so they travel embedded inside the `.mox` RON
itself rather than as a separate file - closer to Lottie's per-asset
embed-or-reference flag, or Rive's embed-by-default. Not Bevy's own
`embedded://` - that id already names the compile-time
`embedded_asset!` source; ours needs a different scheme.

- [ ] Register the `mox://` [`AssetSource`](bevy_asset::io::source::AssetSourceBuilder)
      before `DefaultPlugins` (asset sources build when `AssetPlugin`
      does, not after) - one `Dir` for the process's lifetime, reused
      across project loads, not rebuilt per load.
- [ ] Stage a `.mox`'s embedded section into that `Dir` before
      `WorldDeserializer` resolves any `mox://` handle against it -
      resolution just reads whatever's there.
- [ ] A project asset folder as the one root for imported/external
      files; import (copy in) on assignment rather than reference in
      place.
- [ ] Wire `StdMaterialAssetLoader`'s serializer to an actual "save
      this material as a new asset" action - today only
      `moxie_asset/examples/gen_default_material.rs` ever writes a
      `.mat`.
- [ ] A preview panel beside the asset browser's own listing, showing
      whatever asset is currently selected or hovered there.

## Treat multi-item files (`.glb` and similar) as folders in the asset panel

The asset panel (`editor/moxie/src/ui/assets.rs`) already has a folder
concept, but only for real filesystem directories: `FolderRow` /
`BookmarkRow` both lean on `moxie_ui::fold::Foldable` and a live
`fs::read_dir` (`build_children`) to expand a directory into its
children, tracked open/closed by `AssetFoldState`. What decides
whether a *file* is even recognized is the extensions registered in
`moxie_asset::AssetTypes` (`app.asset_type::<T>().extensions(..)`),
and none are registered today; nothing maps `.glb`/`.gltf`, so such a
file currently renders inert and undraggable.

A `.glb` isn't a single asset the way a `.mat` is - it's a small
archive (meshes, materials, lights, cameras, an implicit scene graph).
Making it *browsable* as a folder means the row needs to expand into a
synthetic child list the same way `FolderRow` does, but sourced from
gltf-parsed contents instead of `fs::read_dir`. That's a second, virtual
kind of expandable row alongside the filesystem-backed one -
`FolderRow`'s body-building step (`build_children`) would need a
non-filesystem counterpart that inspects a `Gltf`/`GltfAssetLabel`'s
node list instead of walking a path, while still fitting the same
`Foldable` shell and `AssetFoldState` bookkeeping. No `Gltf`/
`GltfAssetLabel`/`SceneRoot`-from-`bevy::scene` usage exists anywhere
in `editor/` yet - this is greenfield relative to the current asset
loading code. Worth noting: the editor already has its own unrelated
`SceneRoot` marker (`editor/moxie/src/lib.rs`) naming the scene tree's
own root entity - a real gltf `SceneRoot` component would need a
distinguishing name or an explicit path (`bevy::scene::SceneRoot`)
wherever both are in scope.

Each child item (a mesh, a material, a light) still wants its own
`AssetTypes` registration so it can be dragged into a `Handle<T>`
field - a gltf-derived material handle isn't structurally different
from a hand-authored one once loaded.

- [ ] Register `.glb`/`.gltf` extensions against `bevy_gltf::Gltf` (or
      per-sub-asset types) in `AssetTypes`, so the file stops being
      inert in the browser.
- [ ] A "virtual folder" row variant that expands via a gltf's parsed
      node/mesh/material list instead of `fs::read_dir`, reusing
      `Foldable`/`AssetFoldState` rather than forking them.
- [ ] Decide the child-item addressing scheme (bevy's own
      `GltfAssetLabel` path syntax, e.g. `Mesh0/Primitive0`, is the
      obvious fit) so a child row's path round-trips through
      `AssetServer::load`.
- [ ] Wire each recognized child kind (`Handle<Mesh>`,
      `Handle<StandardMaterial>`, lights) into `draggable(...)` the
      same way `file_row` already does for flat files.
- [ ] Generalize past `.glb` specifically once the pattern holds -
      any future container format (e.g. a multi-clip animation file)
      wants the same virtual-folder shell, not a bespoke one per format.

## Drag a `.glb` straight into the hierarchy to spawn its scene

Two unrelated drag systems exist today, and neither reaches across to
the other. `editor/moxie/src/ui/hierarchy/drag.rs`'s `Dragging`
resource only reacts to other hierarchy rows (reparent/reorder within
the tree). `editor/moxie_ui/src/asset.rs`'s `AssetDragging` resource -
structurally the same ghost-follows-cursor pattern, for files instead
of rows - is only ever *read* by one consumer:
`editor/moxie_ui/src/inspector/handle.rs`'s `Inspect for Handle<T>`,
which compares `AssetDragging.kind` against the inspected field's
`TypeId` and loads a `Handle<T>` onto that field on drop. Dropping a
file over the hierarchy panel today does nothing - nothing there reads
`AssetDragging` at all.

Spawning a `.glb`'s scene into the hierarchy needs a new drop target
that *does* read `AssetDragging`, added to the hierarchy panel (rows
and/or the gap strip in `editor/moxie/src/ui/hierarchy.rs`), mirroring
`Inspect for Handle<T>`'s drop-handling shape but building
`bevy::scene::SceneRoot`/child entities from the gltf's default scene
instead of writing a single field. This is a natural companion to the
folder-browsing item above (same `.glb` registration work), but is
useful on its own even before individual gltf children are browsable -
dropping the whole file can just spawn the default scene.

- [ ] A hierarchy-panel drop target reading `AssetDragging`,
      alongside the existing entity-reparenting one in
      `hierarchy/drag.rs` - same resource, different consumer.
- [ ] On drop, resolve the dragged path to a gltf asset and spawn its
      default scene (or the whole node graph) as children of the drop
      target, parallel to how `Inspect for Handle<T>` resolves a path
      through `AssetServer` with `override_unapproved()` today.
- [ ] Decide undo/naming conventions for a spawned subtree - it's the
      first hierarchy mutation that isn't either hand-built in the
      editor or loaded wholesale from a `.mox`.

## Multiple simultaneous tracks in the timeline

"Multiple tracks" already exists at two different layers, and neither
is a video-editor-style stack a user can freely add to:

- **Runtime** (`crates/motiongfx/src/track.rs`,`timeline.rs`):
  `TrackList`/`Timeline` do hold `Box<[Track]>`, but `curr_index`/
  `target_index` and `set_target_track` describe *switching between*
  tracks (jump to another sequence and play toward/from it), not
  sampling several simultaneously. Repurposing this for layered
  playback would change `queue_actions`'s sampling model, not just add
  UI.
- **Editor UI** (`motiongfx_scene::block::Block`/`Combinator`,
  `editor/moxie/src/block_layout.rs`): `Scene::animation` is one
  `Block` tree; "tracks" only appear as the lanes `block_layout::layout`
  already assigns per sibling under `All`/`Any`/`Flow` - every child of
  one of those combinators gets its own row today
  (`measure_children`), stacked and drawn by `ui/timeline.rs`'s
  `TrackArea`. There's no persistent, user-named, independently
  addable "Track 1 / Track 2" the way a video editor has - what looks
  like stacked lanes is really nested combinator structure, laid out
  automatically.

The second point matters for scoping: a literal free-form track list
(add/remove/reorder independent tracks a user names) means either
changing `Scene`'s schema to hold a flat `Vec<Block>` alongside/instead
of the single `animation` root - touching serialization,
`block_layout.rs`, and every `EditorScene` mutator - or leaning on the
`All`/`Flow` nesting that already lays siblings out as lanes and
building add/remove/reorder affordances directly on top of that
existing behavior. The latter is a much smaller lift since the layout
half is already built; it would mainly need UI to let a user insert/
remove a top-level `All` child and treat it as a named lane, rather
than requiring the tree-editing gestures the timeline doesn't expose
today.

- [ ] Decide the schema question first: reuse `All`/`Flow` top-level
      children as lanes, or give `Scene` an explicit flat track list -
      this determines the size of everything else.
- [ ] If reusing combinator nesting: UI affordances on the timeline
      panel to add/remove/reorder a top-level lane (today's tree has
      no user-facing "insert a sibling block" gesture at all).
- [ ] Lane naming/labeling - `block_layout.rs`'s `combinator_label`
      shows the `Combinator` kind, not a user-chosen name; a track
      stack wants the latter.
- [ ] If simultaneous (not switched) playback is ever needed at
      runtime, not just visually stacked in the editor: revisit
      `Timeline`'s `curr_index`/`target_index` sampling model in
      `crates/motiongfx/src/timeline.rs`, which currently assumes one
      active track at a time.

## More for `fynix_macros`

`fynix_macros` is back with `#[element]`, which writes an element's
builder methods, `Styled`, `Layered`, `Element` and its `{Struct}Props`
trait from the struct, with each prop written through its own lenz
tag. What is still written by hand:

- [ ] The patch types. Each prop names a unit struct with a `Patch`
      impl, made with `patch!`, `node_patch!` or `size_patch!`. Taking
      a closure in the attribute (`#[elem(write = |ui, v| ..)]`) and
      generating the struct would put the write beside its prop.
- [ ] The one-rule `when` on an element, still made by `own_when!`.
      `#[element]` could write it, if it learned the state type the
      backend uses.
- [ ] Per-prop transitions by name. Every blending prop travels over
      the one curve a transition rule gives. With props keyed by
      `FieldId`, `.transition(..)` could name the props it is for.
- [ ] `Styled` and `Layered` for a composite, which still needs
      `styled!` or a hand-written impl to take path rules.
