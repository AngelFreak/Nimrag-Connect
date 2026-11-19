//! FIT profile lookups (names, scale/offset, units, enums). Builder B generates
//! the data (stage 1b). Contract:
//!
//! pub struct FieldInfo { pub name: &'static str, pub base_type: &'static str,
//!     pub type_name: &'static str, pub scale: Option<f64>, pub offset: Option<f64>,
//!     pub units: Option<&'static str>, pub array: bool }
//! pub fn message_name(global: u16) -> Option<&'static str>
//! pub fn message_number(name: &str) -> Option<u16>
//! pub fn field(global: u16, num: u8) -> Option<&'static FieldInfo>
//! pub fn field_by_name(global: u16, name: &str) -> Option<(u8, &'static FieldInfo)>
//! pub fn enum_name(type_name: &str, value: u64) -> Option<&'static str>
