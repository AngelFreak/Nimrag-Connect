//! Device catalogue: recognition by product id and advertised name, table invariants.

use nimrag_core::devices::{
    by_name, by_product_id, identify, model_for_product, pattern, DeviceKind, GARMIN_COMPANY_ID,
    GENERIC, MODELS,
};

#[test]
fn instinct2_by_product_id_is_verified() {
    let m = by_product_id(3888).expect("3888 is the Instinct 2");
    assert_eq!(m.id, "GARMIN_INSTINCT_2");
    assert_eq!(m.name, "Garmin Instinct 2");
    assert!(m.verified);
    assert_eq!(m.kind, DeviceKind::Watch);
}

#[test]
fn instinct2_by_advertised_name() {
    let m = by_name("Instinct 2").expect("name form from PROTOCOL.md");
    assert_eq!(m.id, "GARMIN_INSTINCT_2");
    assert!(m.verified);
}

#[test]
fn only_instinct2_is_verified() {
    let verified: Vec<_> = MODELS.iter().filter(|m| m.verified).map(|m| m.id).collect();
    assert_eq!(verified, ["GARMIN_INSTINCT_2"]);
}

#[test]
fn other_models_by_name() {
    assert_eq!(by_name("Edge 530").unwrap().kind, DeviceKind::BikeComputer);
    assert_eq!(by_name("Venu 3").unwrap().kind, DeviceKind::Watch);
    assert_eq!(
        by_name("Instinct 2S Solar").unwrap().id,
        "GARMIN_INSTINCT_2S_SOLAR"
    );
    assert!(
        by_name("Instinct 2X Solar")
            .unwrap()
            .features
            .solar_charging
    );
    assert_eq!(
        by_name("Instinct 2X Sol").unwrap().id,
        by_name("Instinct 2X Solar").unwrap().id
    );
    assert_eq!(
        by_name("HRM600:12345").unwrap().kind,
        DeviceKind::ChestStrap
    );
    assert_eq!(by_name("inReach Mini 2").unwrap().kind, DeviceKind::Phone);
}

#[test]
fn other_models_by_product_id() {
    for m in MODELS.iter().filter(|m| !m.product_ids.is_empty()).take(10) {
        for id in m.product_ids {
            assert_eq!(by_product_id(*id).unwrap().id, m.id);
        }
    }
}

#[test]
fn names_with_size_and_serial_suffixes() {
    assert_eq!(
        by_name("Venu 4 - 45mm").unwrap().id,
        by_name("Venu 4 41mm").unwrap().id
    );
    assert_eq!(
        by_name("Instinct E - 40mm").unwrap().id,
        "GARMIN_INSTINCT_E"
    );
    assert!(by_name("Montana 700 #123").is_some());
    assert!(by_name("Montana 700").is_some());
    assert_eq!(
        by_name("Venu 3S").unwrap().id,
        by_name("UAT-4261").unwrap().id
    );
}

#[test]
fn name_matching_is_whole_name_and_case_sensitive() {
    assert!(by_name("instinct 2").is_none());
    assert!(by_name("INSTINCT 2").is_none());
    assert!(by_name("Instinct 2 ").is_none());
    assert!(by_name("My Instinct 2").is_none());
    assert!(by_name("Instinct 2X Foo").is_none());
    assert!(by_name("").is_none());
}

#[test]
fn unknown_device_is_generic_never_panics() {
    assert!(by_product_id(0).is_none());
    assert!(by_product_id(u16::MAX).is_none());
    assert_eq!(model_for_product(u16::MAX).id, GENERIC.id);
    assert_eq!(identify(Some("Not A Garmin"), None).id, "GARMIN_GENERIC");
    assert_eq!(identify(None, None).id, "GARMIN_GENERIC");
    assert_eq!(
        identify(Some("\u{0}\u{ffff}"), Some(&[])).id,
        "GARMIN_GENERIC"
    );
    assert!(!GENERIC.verified);
    assert_eq!(GENERIC.kind, DeviceKind::Unknown);
}

#[test]
fn manufacturer_data_decides_before_name() {
    assert_eq!(GARMIN_COMPANY_ID, 0x0087);
    let data = [0x0F, 0x30, 0xAA]; // 3888 big endian, then extra bytes
    assert_eq!(identify(None, Some(&data)).id, "GARMIN_INSTINCT_2");
    // Gadgetbridge does not fall back to the name once product data is present.
    assert_eq!(
        identify(Some("Instinct 2"), Some(&[0xFF, 0xFF])).id,
        "GARMIN_GENERIC"
    );
    // Fewer than two bytes: name fallback.
    assert_eq!(
        identify(Some("Instinct 2"), Some(&[0x0F])).id,
        "GARMIN_INSTINCT_2"
    );
}

#[test]
fn table_invariants() {
    assert!(MODELS.len() >= 190, "ported {} models", MODELS.len());
    let mut ids = std::collections::HashSet::new();
    let mut products = std::collections::HashMap::new();
    for m in MODELS {
        assert!(ids.insert(m.id), "duplicate model id {}", m.id);
        assert!(!m.name.is_empty() && !m.name_pattern.is_empty(), "{}", m.id);
        for p in m.product_ids {
            if let Some(prev) = products.insert(*p, m.id) {
                panic!("product {p} maps to both {prev} and {}", m.id);
            }
        }
    }
}

#[test]
fn every_pattern_is_supported_and_has_a_matching_name() {
    // A pattern outside the supported subset would silently never match: catch that
    // by requiring every model to match at least one plausible name of its own.
    for m in MODELS {
        let plain = m.name_pattern.trim_start_matches('^').trim_end_matches('$');
        let candidate = plain
            .replace("( - )?\\d+mm", " 45mm")
            .replace("( - \\d+mm)?", "")
            .replace("( \\d+mm)?", "")
            .replace("( - [\\d+]+mm)?", "")
            .replace("( [\\d+]+mm)?", "")
            .replace("( #\\d+)?", "");
        if candidate.contains(['(', '[', '\\', '|', '?', '*', '+']) {
            continue; // handled by dedicated cases above
        }
        assert!(
            pattern::is_match(m.name_pattern, &candidate),
            "{}: pattern {:?} does not match {:?}",
            m.id,
            m.name_pattern,
            candidate
        );
    }
}

#[test]
fn pattern_subset() {
    assert!(pattern::is_match("^a(b|c)?d+$", "ad"));
    assert!(pattern::is_match("^a(b|c)?d+$", "acddd"));
    assert!(!pattern::is_match("^a(b|c)?d+$", "abcd"));
    assert!(pattern::is_match("^x[+]\\d+$", "x+12"));
    assert!(pattern::is_match("^v *3$", "v3") && pattern::is_match("^v *3$", "v   3"));
    assert!(pattern::is_match("^A|B$", "A") && pattern::is_match("^A|B$", "B"));
    assert!(!pattern::is_match("^A|B$", "AB"));
    assert!(pattern::is_match("[0-9]+[sS]", "12S"));
    assert!(pattern::is_match("a\\+b", "a+b"));
    assert!(
        !pattern::is_match("a.b", "axb"),
        "unsupported syntax never matches"
    );
}

#[test]
fn feature_flags_follow_gadgetbridge_hierarchy() {
    let watch = by_name("Instinct 2").unwrap();
    assert!(watch.features.spo2 && watch.features.stress && !watch.features.solar_charging);
    assert!(by_name("Instinct 2 Solar").is_some_and(|m| m.features.solar_charging));
    let bike = by_name("Edge 530").unwrap();
    assert!(bike.features.recorded_activities);
}

#[test]
fn sync_support_instinct2_supported() {
    use nimrag_core::devices::SyncSupport;
    assert_eq!(by_name("Instinct 2").unwrap().sync, SyncSupport::Supported);
    assert_eq!(by_product_id(3888).unwrap().sync, SyncSupport::Supported);
}

#[test]
fn sync_support_new_sync_models_cannot_sync_yet() {
    use nimrag_core::devices::SyncSupport;
    for name in [
        "Venu 4 - 45mm",
        "CIRQA Smart Band",
        "Venu X1",
        "vívoactive 6",
    ] {
        let m = by_name(name).unwrap_or_else(|| panic!("{name}"));
        assert!(
            matches!(m.sync, SyncSupport::CannotSyncYet(r) if r.contains("new file-sync")),
            "{name}"
        );
    }
    let n = MODELS
        .iter()
        .filter(|m| m.sync != SyncSupport::Supported)
        .count();
    assert_eq!(n, 5, "D2 Air X15 inherits it from Venu X1");
}
