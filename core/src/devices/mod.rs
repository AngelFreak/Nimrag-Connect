//! Garmin device catalogue: which models exist, how to recognise them, what they can do.
//!
//! Ported from Gadgetbridge (commit 7257742b, AGPL-3.0-or-later); see `catalogue.rs`
//! for the source paths and original copyright lines, and docs/PORTED.md.
//! Every entry except the Instinct 2 is "supported per Gadgetbridge, not verified".
//!
//! How Gadgetbridge recognises a Garmin model (`GarminCoordinator.supports`):
//! 1. If the BLE advertisement carries manufacturer data for company 0x0087 (Garmin)
//!    with at least 2 bytes, the first two bytes (big endian) are the Garmin product
//!    number. It is looked up in a product-number table (`GarminProductNumbers`,
//!    built from the FIT product enum) that maps to one model. The name is then
//!    NOT consulted.
//! 2. Otherwise the advertised name must fully match the model's Java regex
//!    (`Pattern.matches`, case sensitive). Models are tried in `DeviceType` order.
//!
//! The regexes Gadgetbridge uses need only literals, escapes (`\d`, `\+`),
//! classes (`[+]`, `[0-9]`, `[\d+]`, `[sS]`), groups, `|`, `?`, `*`, `+` and the
//! anchors `^` `$`. [`pattern::is_match`] implements exactly that, std only; the
//! pattern strings in the table are stored unchanged.

mod catalogue;
pub mod pattern;

pub use catalogue::MODELS;

/// Bluetooth SIG company identifier of Garmin International, Inc.
pub const GARMIN_COMPANY_ID: u16 = 0x0087;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Unknown,
    Watch,
    FitnessBand,
    BikeComputer,
    ChestStrap,
    /// Gadgetbridge files the inReach satellite messenger under "phone".
    Phone,
}

/// Device-describing capability flags from Gadgetbridge's coordinators. Flags a
/// coordinator computes at run time (training status, training readiness) are not
/// included. Android-UI-only methods are skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    pub heart_rate: bool,
    pub manual_heart_rate: bool,
    pub spo2: bool,
    pub respiratory_rate: bool,
    pub stress: bool,
    pub body_energy: bool,
    pub hrv: bool,
    pub sleep_score: bool,
    /// Gadgetbridge reuses its PAI slot for Garmin intensity minutes.
    pub intensity_minutes: bool,
    pub vo2_max: bool,
    pub vo2_max_multisport: bool,
    pub race_prediction: bool,
    pub training_load: bool,
    pub solar_charging: bool,
    pub weather: bool,
    pub calendar_events: bool,
    pub call_control: bool,
    pub recorded_activities: bool,
    pub data_fetching: bool,
}

impl Features {
    pub const NONE: Features = Features {
        heart_rate: false,
        manual_heart_rate: false,
        spo2: false,
        respiratory_rate: false,
        stress: false,
        body_energy: false,
        hrv: false,
        sleep_score: false,
        intensity_minutes: false,
        vo2_max: false,
        vo2_max_multisport: false,
        race_prediction: false,
        training_load: false,
        solar_charging: false,
        weather: false,
        calendar_events: false,
        call_control: false,
        recorded_activities: false,
        data_fetching: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// Gadgetbridge `DeviceType` constant, e.g. `GARMIN_INSTINCT_2`.
    pub id: &'static str,
    pub name: &'static str,
    /// Java regex matched against the whole advertised BLE name.
    pub name_pattern: &'static str,
    /// Garmin product numbers Gadgetbridge maps to this model (5024 `product`).
    pub product_ids: &'static [u16],
    pub kind: DeviceKind,
    pub features: Features,
    /// 0 means the device does not report a battery level (Gadgetbridge default is 1).
    pub battery_count: u8,
    /// Gadgetbridge marks the model experimental.
    pub experimental: bool,
    /// True only where our own captures verify the model.
    pub verified: bool,
}

/// Returned for anything unrecognised: a Garmin device with no known features.
pub static GENERIC: Model = Model {
    id: "GARMIN_GENERIC",
    name: "Garmin device",
    name_pattern: "",
    product_ids: &[],
    kind: DeviceKind::Unknown,
    features: Features::NONE,
    battery_count: 1,
    experimental: true,
    verified: false,
};

/// Look up by Garmin product number (the `product` field of 5024 DeviceInformation).
pub fn by_product_id(product: u16) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.product_ids.contains(&product))
}

/// Look up by advertised BLE name (whole-name, case-sensitive regex match, first hit).
pub fn by_name(name: &str) -> Option<&'static Model> {
    MODELS
        .iter()
        .find(|m| pattern::is_match(m.name_pattern, name))
}

/// Product number from Garmin manufacturer data (company 0x0087): first two bytes,
/// big endian. `data` excludes the company id itself.
pub fn product_from_manufacturer_data(data: &[u8]) -> Option<u16> {
    match data {
        [hi, lo, ..] => Some(u16::from(*hi) << 8 | u16::from(*lo)),
        _ => None,
    }
}

/// Recognise a device from its advertisement the way Gadgetbridge does: Garmin
/// manufacturer data (if present) decides by product number, otherwise the name.
/// Never fails: unknown devices yield [`GENERIC`].
pub fn identify(name: Option<&str>, manufacturer_data: Option<&[u8]>) -> &'static Model {
    if let Some(product) = manufacturer_data.and_then(product_from_manufacturer_data) {
        return by_product_id(product).unwrap_or(&GENERIC);
    }
    name.and_then(by_name).unwrap_or(&GENERIC)
}

/// Like [`by_product_id`] but falls back to [`GENERIC`].
pub fn model_for_product(product: u16) -> &'static Model {
    by_product_id(product).unwrap_or(&GENERIC)
}
