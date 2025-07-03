use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::antispam::now_ts_secs;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SpamNames(HashMap<String, Encounter>);

static NEVER_STALE_DAYS: u64 = 28;
static RETURNER_STALE_DAYS: u64 = 90;

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
            let days = (now_ts_secs() - encounter.last_seen_ts_secs) / (3600 * 24);
            let retain =
                days <= NEVER_STALE_DAYS || encounter.count > 1 && days <= RETURNER_STALE_DAYS;
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

#[test]
fn test_stale_cleanup() {
    let mut names = SpamNames::default();
    let now = now_ts_secs();
    let d7 = now_ts_secs() - 7 * 3600 * 24;
    let d30 = now_ts_secs() - 30 * 3600 * 24;
    let d100 = now_ts_secs() - 100 * 3600 * 24;
    let mut insert = |name: &str, count, last_seen_ts_secs| {
        names.0.insert(
            name.to_string(),
            Encounter {
                count,
                first_seen_ts_secs: 0,
                last_seen_ts_secs,
            },
        )
    };
    insert("now_1", 1, now);
    insert("d7_1", 1, d7);
    insert("d30_1", 1, d30); // stale
    insert("d30_2", 2, d30);
    insert("d100_9", 9, d100); // stale
    names.cleanup_stale_entries();

    assert!(names.has_encountered("now_1"));
    assert!(names.has_encountered("d7_1"));
    assert!(!names.has_encountered("d30_1"));
    assert!(names.has_encountered("d30_2"));
    assert!(!names.has_encountered("d100_9"));
}
