use std::time::Duration;

use teloxide::Bot;
use tokio::time::MissedTickBehavior;

use crate::storage::Storage;

static CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
pub struct BackgroundSpamCheck {
    bot: Bot,
    storage: Storage,
}

impl BackgroundSpamCheck {
    pub fn new(bot: Bot, storage: Storage) -> Self {
        Self { bot, storage }
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
        // TODO
        Ok(())
    }
}
