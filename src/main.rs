use ahgroupbot::{Actions, PolicyState};
use futures::StreamExt;
use log::{debug, info, warn};
use std::{env, time::Duration};
use teloxide::{dispatching::update_listeners::polling_default, prelude::*, RequestError};
use tokio::time::sleep;

// Avoid unlimited concurrent requests sending to Telegram server.
// Not sure if it is necessary, set as a safeguard anyway.
const MAX_OUTSTANDING_REQUESTS: usize = 30;

const MAX_RETRY: u32 = 5;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let bot = Bot::new(token);
    let actions = Actions::new(&bot, MAX_OUTSTANDING_REQUESTS, MAX_RETRY);
    let mut policy = PolicyState::new();
    let mut stream = Box::pin(polling_default(bot.clone()));
    let mut retry_count = 0u32;
    info!("AhGroupBot started");
    while let Some(update) = stream.next().await {
        debug!("Update: {:?}", update);
        let update = match update {
            Ok(update) => {
                retry_count = 0;
                update
            }
            Err(RequestError::NetworkError(err)) if retry_count < MAX_RETRY => {
                warn!("Netwrok error: {}", err);
                sleep(RETRY_BASE_DELAY * 2u32.pow(retry_count)).await;
                retry_count += 1;
                continue;
            }
            Err(err) => return Err(err.into()),
        };
        if let Some((chat_id, msg_id)) = policy.get_message_to_delete(update) {
            actions.spwan_delete_message(chat_id, msg_id).await;
        }
    }
    Ok(())
}
