use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use teloxide::{Bot, prelude::Requester, types::ChatId};
use tokio::time::MissedTickBehavior;

use crate::{Actions, SpamState, storage::Storage};

static CHECK_INTERVAL: Duration = Duration::from_secs(10 * 60);
static MIN_AUTHENTIC_USERS: usize = 10;
static UID_PERCENTILE: f32 = 98.0;
static NEW_USER_GRACE_TIME: Duration = Duration::from_secs(30 * 60);

#[derive(Debug)]
pub struct BackgroundSpamCheck {
    bot: Bot,
    storage: Storage,
    actions: Actions,
    cid: ChatId,
}

impl BackgroundSpamCheck {
    pub fn new(bot: Bot, storage: Storage, actions: Actions, cid: ChatId) -> Self {
        Self {
            bot,
            storage,
            actions,
            cid,
        }
    }

    pub async fn launch(self) -> ! {
        let mut interval = tokio::time::interval(CHECK_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(err) = self.check_spam().await {
                log::warn!("Error on background spam check: {err}");
            }
        }
    }

    async fn check_spam(&self) -> anyhow::Result<()> {
        log::debug!("Background spam check");
        // Get list of authentic user
        let uids: Vec<_> = self
            .storage
            .with_user_states(|user_states| {
                user_states
                    .filter(|(_, state)| state.is_authentic())
                    .map(|(uid, _)| uid.0)
                    .collect()
            })
            .await;
        if uids.len() < MIN_AUTHENTIC_USERS {
            log::debug!("Skip check: authentic users < {MIN_AUTHENTIC_USERS}");
            return Ok(());
        }
        // Anyone with uid < safe_uid are safe (unlikey be spam)
        let safe_uid = percentile(UID_PERCENTILE, uids).unwrap();
        let grace_ts = (SystemTime::now() - NEW_USER_GRACE_TIME)
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        let suspect_uids: Vec<_> = self
            .storage
            .with_user_states(|user_states| {
                user_states
                    .filter_map(|(uid, state)| match state {
                        SpamState::MaybeSpam { create_ts_secs, .. }
                            if uid.0 > safe_uid && *create_ts_secs < grace_ts =>
                        {
                            Some(*uid)
                        }
                        _ if state.is_spam() => Some(*uid),
                        _ => None,
                    })
                    .collect()
            })
            .await;
        // Ban in all chats
        log::debug!("Safe UID: <{safe_uid}; suspect user: {suspect_uids:?}");
        for uid in suspect_uids {
            self.storage.update_user(&uid, SpamState::new_spam()).await;
            if let Ok(member) = self.bot.get_chat_member(self.cid, uid).await {
                // Ban user
                if member.is_present() {
                    self.actions.spawn_ban_user(self.cid, uid).await;
                } else {
                    self.storage.remove_user(&uid).await;
                }
                // Update spam name list
                self.storage
                    .with_spam_names(|names| names.encounter(member.user.full_name()))
                    .await;
            }
        }
        // TODO: reduce to once a day
        self.storage
            .with_spam_names(|names| names.cleanup_stale_entries())
            .await;
        self.storage.save().await?;
        Ok(())
    }
}

/// Get `k`-th percentile from `nums`.
/// Optizimed for `k` > 50. No duplicate items in `nums`.
/// Panic if `k` is not in [0, 100]
fn percentile<N: Ord>(k: f32, nums: Vec<N>) -> Option<N> {
    if !(0.0..=100.0).contains(&k) {
        panic!("k = {k} is not in [0, 100]");
    }
    // We only optimize for `k` > 50, hence keep the max some items.
    let top_n = ((1.0 - k / 100.0) * (nums.len() as f32)).round() as usize;
    let top_n = top_n.clamp(1, usize::MAX);
    let mut tops = BTreeSet::new();
    // Find the minimal item among the top-n items.
    for num in nums {
        if tops.len() < top_n {
            tops.insert(num);
        } else if let Some(bottom) = tops.first() {
            if num > *bottom {
                tops.pop_first();
                tops.insert(num);
            }
        }
    }
    tops.pop_first()
}

#[test]
fn test_percentile() {
    use rand::seq::SliceRandom;

    let mut rng = rand::rng();
    let mut n = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    n.shuffle(&mut rng);

    assert_eq!(percentile(0.0, Vec::<u8>::new()), None);
    assert_eq!(percentile(0.0, n.clone()), Some(0));
    assert_eq!(percentile(100.0, n.clone()), Some(9));
    assert_eq!(percentile(50.0, n.clone()), Some(5));
    assert_eq!(percentile(84.0, n), Some(8));
}
