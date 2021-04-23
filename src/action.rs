use log::{info, warn};
use std::sync::Arc;
use teloxide::{
    requests::{Request, Requester},
    Bot,
};
use tokio::sync::Semaphore;

use crate::{ChatId, MessageId};

pub struct Actions {
    bot: Bot,
    outstanding_limit: Arc<Semaphore>,
}

impl Actions {
    pub fn new(bot: &Bot, max_outstanding_requests: usize) -> Self {
        Self {
            bot: bot.clone(),
            outstanding_limit: Arc::new(Semaphore::new(max_outstanding_requests)),
        }
    }

    /// Spawn a new task to delete the message.
    /// If outstanding request limit reached, wait for it before spwan and return.
    pub async fn spwan_delete_message(&self, chat_id: ChatId, msg_id: MessageId) {
        let permit = self.outstanding_limit.clone().acquire_owned().await;
        let bot = self.bot.clone();
        tokio::spawn(async move {
            info!("Deleting [{}:{}]", chat_id, msg_id);
            if let Err(err) = bot.delete_message(chat_id, msg_id).send().await {
                warn!("Fail to delete [{}:{}]: {:?}", chat_id, msg_id, err);
            }
            drop(permit);
        });
    }
}
