use super::*;
use crate::design::ShapeKind;

fn project(id: &str, kind: ShapeKind) -> Project {
    let mut design = Design::default();
    design.add_shape(kind);
    Project { id: id.into(), name: id.into(), design, created: 1, modified: 1 }
}

fn version(at: u64, kind: VersionKind) -> Version {
    Version { id: VersionId(at), kind, name: None, pinned: false }
}

#[test]
fn an_interrupted_write_leaves_the_previous_file_intact_and_readable() {
    let dir = tempfile::tempdir().unwrap();
    let first = project("p", ShapeKind::Star);
    project::save(dir.path(), &first).unwrap();
    let path = project::json_path(dir.path(), "p");
    let before = std::fs::read_to_string(&path).unwrap();

    let changed = project("p", ShapeKind::Heart);
    let staged = stage(&path, project::to_json(&changed).unwrap().as_bytes()).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "the target is untouched until the rename");
    assert_eq!(project::open(dir.path(), "p").unwrap().0.design, first.design, "and still opens");
    drop(staged);
    assert_eq!(project::list(dir.path()).len(), 1, "the stray temp file is not listed as a project");
}

#[test]
fn a_committed_write_replaces_the_file_and_leaves_no_temp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.json");
    write_atomically(&path, b"one").unwrap();
    write_atomically(&path, b"two").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"two");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// An Auto version every 30 minutes for 60 days, `now` on a UTC day boundary, with one Manual and one
/// pinned version among them. `k` counts half hours back from now.
fn sixty_days() -> (Index, VersionId) {
    let now = 20_000 * DAY_MS;
    let half_hour = HOUR_MS / 2;
    let versions = (0..=2880u64)
        .rev()
        .map(|k| {
            let mut v = version(now - k * half_hour, if k == 1920 { VersionKind::Manual } else { VersionKind::Auto });
            v.pinned = k == 2000;
            v
        })
        .collect();
    (Index { versions }, VersionId(now))
}

#[test]
fn prune_keeps_exactly_the_documented_set_over_sixty_days() {
    let (index, now) = sixty_days();
    let gone: HashSet<VersionId> = prune(&index, now).into_iter().collect();
    let kept: Vec<u64> = index.versions.iter().filter(|v| !gone.contains(&v.id)).map(|v| (now.0 - v.id.0) / (HOUR_MS / 2)).collect();

    let last_hour = [0, 1, 2];
    // Hour buckets pair half hours (k = 3 with 4, 5 with 6, ...); the newer of each pair stays.
    let hourly = (3..=47).step_by(2);
    // Day buckets hold 48 half hours; the newest in each is the first past a day boundary.
    let daily = (2..=30).map(|d| 48 * (d - 1) + 1);
    let manual_and_pinned = [1920, 2000];
    let mut expected: Vec<u64> = last_hour.into_iter().chain(hourly).chain(daily).chain(manual_and_pinned).collect();
    expected.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(kept, expected);
}

#[test]
fn pruning_twice_changes_nothing() {
    let (mut index, now) = sixty_days();
    let gone = prune(&index, now);
    index.versions.retain(|v| !gone.contains(&v.id));
    assert_eq!(prune(&index, now), []);
}

#[test]
fn prune_caps_a_project_at_the_newest_hundred() {
    let index = Index { versions: (0..150).map(|i| version(1_000 + i, VersionKind::Manual)).collect() };
    let gone = prune(&index, VersionId(2_000));
    assert_eq!(gone, (1_000..1_050).map(VersionId).collect::<Vec<_>>());
}

#[test]
fn manual_versions_go_after_ninety_days() {
    let now = 200 * DAY_MS;
    let index = Index { versions: vec![version(now - 91 * DAY_MS, VersionKind::Manual), version(now - 89 * DAY_MS, VersionKind::Manual)] };
    assert_eq!(prune(&index, VersionId(now)), [VersionId(now - 91 * DAY_MS)]);
}

#[test]
fn versions_record_with_thumbnails_and_ids_only_grow() {
    let dir = tempfile::tempdir().unwrap();
    let p = project("p", ShapeKind::Star);
    let a = record_version(dir.path(), &p, VersionKind::Auto, 5_000).unwrap();
    let b = record_version(dir.path(), &p, VersionKind::Manual, 5_000).unwrap();
    let c = record_version(dir.path(), &p, VersionKind::Auto, 4_000).unwrap();
    assert_eq!((a, b, c), (VersionId(5_000), VersionId(5_001), VersionId(5_002)));
    assert!(version_png(dir.path(), "p", a).exists());
    assert_eq!(read_version(dir.path(), "p", b).unwrap().design, p.design);

    name_version(dir.path(), "p", a, "Before the heart".into()).unwrap();
    let index = read_index(dir.path(), "p").unwrap();
    assert_eq!(index.versions.iter().map(|v| v.id).collect::<Vec<_>>(), [a, b, c]);
    assert_eq!((index.versions[0].name.as_deref(), index.versions[0].pinned), (Some("Before the heart"), true));
}

#[test]
fn restore_records_before_restore_and_restoring_that_returns_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let old = project("p", ShapeKind::Star);
    let older = record_version(dir.path(), &old, VersionKind::Manual, 1_000).unwrap();
    let current = project("p", ShapeKind::Heart);

    let restored = restore(dir.path(), &current, older, 2_000).unwrap();
    assert_eq!(restored.design, old.design);
    let index = read_index(dir.path(), "p").unwrap();
    let before = index.versions.last().unwrap();
    assert_eq!(before.kind, VersionKind::BeforeRestore);

    let back = restore(dir.path(), &restored, before.id, 3_000).unwrap();
    assert_eq!(back.design, current.design, "undoing the restore from history");
}

#[test]
fn discard_moves_the_autosave_into_versions() {
    let dir = tempfile::tempdir().unwrap();
    let saved = project("p", ShapeKind::Star);
    save_project(dir.path(), &saved, 1_000).unwrap();
    let edited = project("p", ShapeKind::Heart);
    write_autosave(dir.path(), &edited).unwrap();

    discard_autosave(dir.path(), "p", 2_000).unwrap();
    assert!(read_autosave(dir.path(), "p").unwrap().is_none());
    let index = read_index(dir.path(), "p").unwrap();
    let last = index.versions.last().unwrap();
    assert_eq!((last.id, last.kind), (VersionId(2_000), VersionKind::Auto));
    assert_eq!(read_version(dir.path(), "p", last.id).unwrap().design, edited.design);
    discard_autosave(dir.path(), "p", 3_000).unwrap();
    assert_eq!(read_index(dir.path(), "p").unwrap().versions.len(), 2, "a second discard has nothing to move");
}

#[test]
fn save_records_a_manual_version_and_clears_the_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let p = project("p", ShapeKind::Star);
    write_autosave(dir.path(), &p).unwrap();
    save_project(dir.path(), &p, 1_000).unwrap();
    assert!(read_autosave(dir.path(), "p").unwrap().is_none());
    assert_eq!(read_index(dir.path(), "p").unwrap().versions, [version(1_000, VersionKind::Manual)]);
    assert_eq!(project::list(dir.path()).len(), 1, "versions and autosaves are not listed as projects");
}

#[test]
fn delete_removes_the_json_thumbnail_versions_and_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let p = project("p", ShapeKind::Star);
    let keep = project("keep", ShapeKind::Heart);
    for each in [&p, &keep] {
        save_project(dir.path(), each, 1_000).unwrap();
        write_autosave(dir.path(), each).unwrap();
    }

    delete_project(dir.path(), "p").unwrap();
    for gone in [project::json_path(dir.path(), "p"), project::thumb_path(dir.path(), "p"), autosave_path(dir.path(), "p"), versions_dir(dir.path(), "p")] {
        assert!(!gone.exists(), "{} remains", gone.display());
    }
    assert!(autosave_path(dir.path(), "keep").exists() && versions_dir(dir.path(), "keep").exists(), "the other project is untouched");
    delete_project(dir.path(), "p").unwrap();

    delete_project(dir.path(), "keep").unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "no empty folders are left");
}

/// The fake extractor: each shape references `<kind>.png`.
fn by_kind(design: &Design) -> Vec<String> {
    design.shapes.iter().map(|s| format!("{:?}.png", s.kind)).collect()
}

#[test]
fn asset_garbage_collection_removes_only_unreferenced_assets() {
    let dir = tempfile::tempdir().unwrap();
    save_project(dir.path(), &project("p", ShapeKind::Star), 1_000).unwrap();
    write_autosave(dir.path(), &project("q", ShapeKind::Heart)).unwrap();
    record_version(dir.path(), &project("p", ShapeKind::Circle), VersionKind::Auto, 2_000).unwrap();
    session::write(dir.path(), &session::Session::default()).unwrap();
    let assets = assets_dir(dir.path());
    std::fs::create_dir_all(&assets).unwrap();
    for file in ["Star.png", "Heart.png", "Circle.png", "Square.png"] {
        std::fs::write(assets.join(file), file).unwrap();
    }

    assert_eq!(collect_asset_garbage(dir.path(), by_kind).unwrap(), [assets.join("Square.png")]);
    let mut left: Vec<_> = std::fs::read_dir(&assets).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
    left.sort();
    assert_eq!(left, ["Circle.png", "Heart.png", "Star.png"]);
}

#[test]
fn asset_garbage_collection_deletes_nothing_past_an_unreadable_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("torn.json"), "{").unwrap();
    std::fs::create_dir_all(assets_dir(dir.path())).unwrap();
    std::fs::write(assets_dir(dir.path()).join("maybe.png"), "").unwrap();
    assert_eq!(collect_asset_garbage(dir.path(), by_kind).unwrap(), Vec::<PathBuf>::new());
    assert!(assets_dir(dir.path()).join("maybe.png").exists());
}

#[test]
fn a_flat_projects_directory_from_before_lists_and_opens_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let old = project("old", ShapeKind::Star);
    project::save(dir.path(), &old).unwrap();
    let before: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();

    let listed = project::list(dir.path());
    assert_eq!(listed.iter().map(|e| e.id()).collect::<Vec<_>>(), ["old"]);
    assert_eq!(project::open(dir.path(), "old").unwrap().0.design, old.design);
    assert_eq!(read_index(dir.path(), "old").unwrap(), Index::default(), "no versions yet");
    assert!(read_autosave(dir.path(), "old").unwrap().is_none());
    let after: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(before, after, "reading writes nothing");
}

fn unsaved_ids(scan: &unsaved::Scan) -> Vec<&str> {
    scan.listed.iter().map(unsaved::Unsaved::id).collect()
}

#[test]
fn an_autosave_is_unsaved_work_only_without_a_project_file_or_an_open_tab() {
    let dir = tempfile::tempdir().unwrap();
    let at = |id: &str, modified: u64| Project { modified, ..project(id, ShapeKind::Star) };
    let now_ms = 100 * DAY_MS;
    let recent = now_ms / 1000 - 60;
    for id in ["older", "newer", "saved", "open"] {
        write_autosave(dir.path(), &at(id, recent - if id == "older" { 10 } else { 0 })).unwrap();
    }
    project::save(dir.path(), &project("saved", ShapeKind::Star)).unwrap();
    std::fs::write(autosave_path(dir.path(), "torn"), b"{").unwrap();

    let scan = unsaved::scan(dir.path(), &["open".to_owned()], now_ms);
    assert_eq!(unsaved_ids(&scan), ["newer", "older", "torn"], "newest first, the unreadable last");
    assert!(matches!(scan.listed[2], unsaved::Unsaved::Unreadable { .. }));
    assert!(scan.expired.is_empty());
}

#[test]
fn unsaved_work_expires_after_thirty_days_and_an_empty_design_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let now_ms = 100 * DAY_MS;
    let edge = |days: u64, extra_ms: u64| (now_ms - days * DAY_MS - extra_ms).div_ceil(1000);
    write_autosave(dir.path(), &Project { modified: edge(30, 0), ..project("thirty", ShapeKind::Star) }).unwrap();
    write_autosave(dir.path(), &Project { modified: edge(30, 2000), ..project("stale", ShapeKind::Star) }).unwrap();
    write_autosave(dir.path(), &Project { design: Design::default(), modified: now_ms / 1000, ..project("empty", ShapeKind::Star) }).unwrap();

    let scan = unsaved::scan(dir.path(), &[], now_ms);
    assert_eq!(unsaved_ids(&scan), ["thirty"], "30 days exactly is still kept");
    assert_eq!(scan.expired, ["empty", "stale"]);
}

#[test]
fn deleting_unsaved_work_never_touches_an_open_or_saved_id() {
    let dir = tempfile::tempdir().unwrap();
    for id in ["gone", "open", "saved"] {
        write_autosave(dir.path(), &project(id, ShapeKind::Star)).unwrap();
    }
    project::save(dir.path(), &project("saved", ShapeKind::Star)).unwrap();
    let open = ["open".to_owned()];

    for id in ["open", "saved"] {
        unsaved::delete(dir.path(), id, &open).unwrap();
        assert!(autosave_path(dir.path(), id).exists(), "{id} is kept");
    }
    unsaved::delete(dir.path(), "gone", &open).unwrap();
    unsaved::delete(dir.path(), "gone", &open).unwrap();
    assert!(!autosave_path(dir.path(), "gone").exists());
    assert!(unsaved::delete(dir.path(), "../x", &open).is_err(), "ids are checked");
}
