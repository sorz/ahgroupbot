use log::{debug, info, warn};
use std::{convert::TryInto, sync::Arc, time::Duration};
use teloxide::{
    requests::{Request, Requester},
    ApiError, Bot, RequestError,
};
use tokio::{sync::Semaphore, time::sleep};

use crate::{ChatId, MessageId};

const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct Actions {
    bot: Bot,
    max_retry: u32,
    outstanding_limit: Arc<Semaphore>,
}

impl Actions {
    pub fn new(bot: &Bot, max_outstanding_requests: usize, max_retry: u32) -> Self {
        Self {
            bot: bot.clone(),
            max_retry,
            outstanding_limit: Arc::new(Semaphore::new(max_outstanding_requests)),
        }
    }

    /// Spawn a new task to delete the message.
    /// If outstanding request limit reached, wait for it before spwan and return.
    pub async fn spwan_delete_message(&self, chat_id: ChatId, msg_id: MessageId) {
        let permit = self
            .outstanding_limit
            .clone()
            .acquire_owned()
            .await
            .unwrap(); // Semaphore never get closed
        let bot = self.bot.clone();
        let max_retry = self.max_retry;
        tokio::spawn(async move {
            info!("Deleting [{}:{}]", chat_id, msg_id);
            if let Err(err) = delete_message(bot, chat_id, msg_id, max_retry).await {
                warn!("Failed to delete [{}:{}]: {:?}", chat_id, msg_id, err);
            }
            drop(permit);
        });
    }
}

async fn delete_message(
    bot: Bot,
    mut chat_id: ChatId,
    msg_id: MessageId,
    max_retry: u32,
) -> Result<(), RequestError> {
    let mut retry: u32 = 0;
    loop {
        match bot.delete_message(chat_id, msg_id).send().await {
            Ok(_) => break Ok(()),
            Err(RequestError::RetryAfter(secs)) if retry < max_retry => {
                warn!("RetryAfter received, retry deleting after {} secs", secs);
                let delay = secs
                    .try_into()
                    .map(Duration::from_secs)
                    .unwrap_or(RETRY_BASE_DELAY);
                sleep(delay).await;
            }
            Err(RequestError::NetworkError(err)) if retry < max_retry => {
                warn!("Delayed deleting due to network error: {}", err);
                sleep(RETRY_BASE_DELAY * 2u32.pow(retry)).await;
            }
            Err(RequestError::MigrateToChatId(new_chat_id)) if retry < max_retry => {
                chat_id = new_chat_id;
            }
            Err(RequestError::ApiError { kind: err, .. })
                if err == ApiError::MessageToDeleteNotFound
                    || err == ApiError::MessageIdInvalid =>
            {
                debug!("Message [{}:{}] is already gone", chat_id, msg_id);
                break Ok(());
            }
            Err(RequestError::ApiError { kind: err, .. })
                if err == ApiError::MessageCantBeDeleted =>
            {
                debug!("No enough rights to delete message in group {}", chat_id);
                break Ok(()); // No treat as error since we the bot onwer can't help with it
            }
            Err(RequestError::ApiError { kind: err, .. })
                if err == ApiError::BotKicked || err == ApiError::ChatNotFound =>
            {
                debug!("Bot was kicked from group {}", chat_id);
                break Ok(()); // No treat as error
            }
            Err(err) => {
                warn!("Failed to delete message [{}:{}]: {}", chat_id, msg_id, err);
                break Err(err);
            }
        }
        retry += 1;
    }
}
