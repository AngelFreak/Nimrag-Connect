// Empty profile table, used when no locally generated profile is present
// (see tools/fit-profile/README.md). All lookups return None.
pub(super) static MESSAGES: &[MessageInfo] = &[];
pub(super) static ENUMS: &[(&str, &[(u64, &str)])] = &[];
pub(super) static VERSION: Option<u32> = None;
