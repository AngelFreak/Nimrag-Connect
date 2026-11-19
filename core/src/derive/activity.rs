//! Activity parser (scaffold stub; filled by the activity builder).
use super::Derived;
use crate::fit::interpret::Interpreted;

/// Activity row types live here (activities, sessions, laps, records, sets, ...).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActivityRows {}

pub fn run(_msgs: &[Interpreted], _out: &mut Derived) {}
