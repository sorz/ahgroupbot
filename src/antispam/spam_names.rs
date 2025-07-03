use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::antispam::now_ts_secs;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SpamNames(HashMap<String, Encounter>);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Encounter {
    count: usize,
    first_seen_ts_secs: u64,
    last_seen_ts_secs: u64,
}

impl Default for Encounter {
    fn default() -> Self {
        Self {
            count: 0,
            first_seen_ts_secs: now_ts_secs(),
            last_seen_ts_secs: now_ts_secs(),
        }
    }
}

impl Encounter {
    fn encounter(&mut self) {
        self.count += 1;
        self.last_seen_ts_secs = now_ts_secs();
    }
}

impl SpamNames {
    pub(crate) fn encounter(&mut self, full_name: String) {
        self.0.entry(full_name).or_default().encounter();
    }

    /// Side effect: update entry with Encounter::enconter()
    pub(crate) fn has_encountered<S: AsRef<str>>(&mut self, full_name: S) -> bool {
        match self.0.get_mut(full_name.as_ref()) {
            Some(entry) => {
                entry.encounter();
                true
            }
            None => false,
        }
    }

    pub(crate) fn cleanup_stale_entries(&mut self) {
        self.0.retain(|full_name, encounter| {
            let days = encounter.last_seen_ts_secs / (3600 * 24);
            let retain = days <= 28 || encounter.count > 1 && days <= 90;
            if !retain {
                log::info!(
                    "Remove stale spam name: {full_name} ({}/{}d)",
                    encounter.count,
                    days
                );
            }
            retain
        });
    }
}
