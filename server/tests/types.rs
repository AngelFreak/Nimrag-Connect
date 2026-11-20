//! The TypeScript types in `web/src/api/generated/` are generated from the Rust
//! request and response structs. This test regenerates them into a temp dir and
//! fails if the committed files differ. Fix: `cargo run -p nimrag-server -- export-types`.

use std::collections::BTreeMap;
use std::path::Path;

fn read_dir(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".ts") {
            out.insert(name, std::fs::read_to_string(e.path()).unwrap());
        }
    }
    out
}

#[test]
fn generated_typescript_is_up_to_date() {
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/src/api/generated");
    let tmp = tempfile::tempdir().unwrap();
    nimrag_server::web_api::export_types(tmp.path()).unwrap();
    let fresh = read_dir(tmp.path());
    let have = read_dir(&committed);
    let hint = "run `cargo run -p nimrag-server -- export-types` and commit web/src/api/generated";

    assert!(!fresh.is_empty(), "nothing was generated");
    assert!(fresh.contains_key("index.ts") && fresh.contains_key("Today.ts"));
    let missing: Vec<_> = fresh.keys().filter(|k| !have.contains_key(*k)).collect();
    let stale: Vec<_> = have.keys().filter(|k| !fresh.contains_key(*k)).collect();
    assert!(
        missing.is_empty(),
        "missing from {}: {missing:?}; {hint}",
        committed.display()
    );
    assert!(stale.is_empty(), "no longer generated: {stale:?}; {hint}");
    for (name, content) in &fresh {
        assert_eq!(
            &have[name], content,
            "{name} differs from the generated one; {hint}"
        );
    }
}

#[test]
fn every_api_type_is_exported() {
    let tmp = tempfile::tempdir().unwrap();
    nimrag_server::web_api::export_types(tmp.path()).unwrap();
    let index = std::fs::read_to_string(tmp.path().join("index.ts")).unwrap();
    for t in [
        "Me",
        "LoginRequest",
        "TokenList",
        "SettingsResponse",
        "SettingsPatch",
        "HomeCards",
        "HomeCard",
        "Today",
        "DayResponse",
        "MovementResponse",
        "CalendarResponse",
        "ActivityList",
        "ActivityDetail",
        "TrackResponse",
        "StrengthResponse",
        "StrengthPatch",
        "SleepNightResponse",
        "SleepNightsResponse",
        "TrendsResponse",
        "MetricRange",
        "TrainingResponse",
        "BodyResponse",
        "NewBodyEntry",
        "BodyEntry",
        "DevicesResponse",
        "StorageResponse",
        "GarminStatus",
        "GarminLoginRequest",
        "GarminLoginResponse",
        "GarminMfaRequest",
        "GarminBackfillRequest",
        "GarminBackfillResponse",
        "GarminImportTokens",
    ] {
        assert!(
            index.contains(&format!("{t}\"")) || index.contains(&format!("{t} ")),
            "{t} missing from index.ts:\n{index}"
        );
    }
}
