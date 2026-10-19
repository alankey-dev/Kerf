# 33 Autosave, version history, crash recovery and tabs

**Wave 6. Depends on 22, 23. Size XL; run as two phases (storage, autosave and recovery first, then tabs).** Kerf today saves only when asked, loses work on a crash, and opens one project at a time. It should never lose work and should let a crafter keep several projects open.

## Problem

`src/project.rs` writes `<id>.json` and `<id>.png` when the user presses Save. Nothing is written otherwise, so a crash or a forced quit loses everything since the last Save. There is no history of earlier saves. `Kerf` in `src/ui/mod.rs` holds exactly one design, selection, history and view, and `chrome::Tab` is only Home or Canvas.

## Goal

Work is saved continuously, earlier versions can be browsed and restored, a crash costs at most a few seconds, and several projects can be open as tabs, each with its own undo.

### Storage layout

Under the projects directory (`project::projects_dir`, or `ODS_PROJECTS_DIR`):

```
projects/
  <id>.json            current saved project (unchanged format)
  <id>.png             its thumbnail
  autosave/<id>.json   latest unsaved state of an open project
  versions/<id>/<unix ms>.json   one version snapshot
  versions/<id>/<unix ms>.png    its thumbnail (220 px)
  versions/<id>/index.json       version list: time, kind (Manual, Auto, BeforeRestore), name, pinned
  session.json         open tabs in order, active tab, and the running app's pid
  assets/              spec 28's asset store
```

- Every write goes to a sibling temporary file and is renamed into place, so a crash mid-write never corrupts a file. Writing the same state twice leaves the same files.
- A never-saved project gets an id when its tab opens, so it autosaves like any other; it appears on Home only once named or saved.

### Autosave

- After any design change, autosave writes `autosave/<id>.json` 2 seconds after the last change, and at least every 30 seconds during continuous editing. It never writes during a drag (`History::observe`'s open gesture).
- Writes run on one saver thread fed by an `mpsc` channel; the UI sends a cloned `Design` and never serialises or touches the disk. One saver thread is the only writer of the projects directory, so two tabs cannot race on a file.
- The title bar's Save state reads "Saved", "Edited" or "Autosaved 10:42". Save still exists: it writes `<id>.json`, records a Manual version and clears the autosave file.
- Closing a tab or quitting with unsaved edits keeps the autosave and asks nothing; the next open of that project offers "Unsaved changes from 10:42: Restore or Discard".

### Version history

- A version is recorded on every Save (Manual), every 10 minutes of editing (Auto), and before any Restore (BeforeRestore).
- **File > Version history** opens a side panel: versions newest first, grouped by day, each with time, kind, thumbnail and an optional name. Selecting a version previews it read-only on the canvas. **Restore** makes it the current design as one undo step, after recording BeforeRestore. **Name** pins a version.
- **Pruning** runs after each new version: keep every version from the last hour, one per hour for the last day, one per day for the last 30 days, and every pinned or Manual version from the last 90 days, up to 100 versions per project. Pruning is a pure function `prune(index, now) -> Vec<VersionId>` and is idempotent. Deleting a project deletes its versions and autosave.
- Asset garbage collection: an asset in `assets/` that no project, autosave or version references is deleted at start-up.

### Crash recovery

- At launch, `session.json` with a pid that is not running means the last session did not quit cleanly. A clean quit removes the pid.
- After an unclean exit, a recovery dialog lists each tab from the session that has an autosave newer than its saved file, with thumbnail and time: **Restore all**, **Restore selected**, **Discard**. Restored tabs reopen in their old order.
- Discarding moves the autosave into the project's versions as an Auto version, so even Discard can be undone from Version history.

### Tabs

- The tab bar shows Home then one tab per open project with its name, an edited dot and a close button. Cmd+T opens a new blank project, Cmd+W closes the active tab, Ctrl+Tab and Ctrl+Shift+Tab cycle, Cmd+1 to Cmd+9 jump.
- `Kerf` splits into app-wide state (machine, transport, settings, projects dir, Home) and `Vec<Document>`, one per tab: design, selection, history, view, tool, project, title, panel state and the Make flow. Each tab has its own undo; Undo never crosses tabs.
- Opening a project that is already open switches to its tab rather than opening a second copy.
- One Make flow runs at a time because there is one machine. Starting Make in another tab while one is running asks to finish or cancel the first.
- The open tabs and their order restore on next launch from `session.json`.

## Files

- **Owns.** `src/store/mod.rs` (new: layout, atomic writes, autosave and version files, index, prune, asset GC), `src/store/saver.rs` (new: saver thread), `src/store/session.rs` (new: session file, pid check, recovery list), `src/ui/document.rs` (new: per-tab `Document`), `src/ui/tab_bar.rs` (new), `src/ui/versions_panel.rs` (new), `src/ui/recovery.rs` (new), `src/ui/tests_spec33.rs` (new).
- **Shared edits.** `src/project.rs` (route save, delete and list through `store`; `delete` also removes versions and autosave), `src/ui/mod.rs` (split `Kerf` into app state and documents; this is the largest edit in the wave, so merge it first and keep others' rebases small), `src/ui/chrome.rs` (tab bar, File menu items, shortcuts through spec 23), `src/ui/home.rs` (open in tab), `src/settings.rs` (nothing new unless needed).

## Acceptance

- [ ] Unit test (tempdir): a change followed by 2 s of quiet produces `autosave/<id>.json`; continuous changes still write within 30 s; nothing is written while a gesture is open.
- [ ] Unit test (tempdir): a write interrupted after the temporary file is written but before the rename leaves the previous file intact and readable.
- [ ] Unit test: `prune` on a synthetic index spanning 60 days keeps exactly the documented set; running it twice changes nothing.
- [ ] Unit test (tempdir): Restore records a BeforeRestore version, and restoring that version returns the original design.
- [ ] Unit test (tempdir): a `session.json` naming a dead pid with two newer autosaves yields a recovery list of two; a clean session yields none.
- [ ] Unit test (tempdir): Discard moves the autosave into versions.
- [ ] Unit test (tempdir): deleting a project removes its JSON, thumbnail, versions and autosave; asset GC removes only unreferenced assets.
- [ ] Unit test: a projects directory from before this spec (flat `<id>.json` files only) lists and opens unchanged.
- [ ] Kittest: open two projects in tabs, edit both, Undo in the second; the first is unchanged.
- [ ] Kittest: open a project already open; the tab count stays the same and its tab becomes active.
- [ ] Kittest: edit, advance the harness clock 3 s, and the autosave file exists in the tempdir; the title shows "Autosaved".
- [ ] Kittest: start a harness with a stale session and autosaves; the recovery dialog lists them and Restore all reopens the tabs.
- [ ] Kittest: open Version history, select an older version, Restore; the canvas shows it and one Undo returns.
- [ ] Snapshots `33-tabs.png` (three tabs with an edited dot), `33-versions.png` (Version history panel), `33-recovery.png` (recovery dialog).
- [ ] README describes autosave, versions and pruning, crash recovery, tabs and the storage layout.
