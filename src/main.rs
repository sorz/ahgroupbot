use ahgroupbot::{Actions, PolicyState};
use futures::StreamExt;
use log::{debug, info};
use std::env;
use teloxide::{dispatching::update_listeners::polling_default, prelude::*};

// Avoid unlimited concurrent requests sending to Telegram server.
// Not sure if it is necessary, set as a safeguard anyway.
const MAX_OUTSTANDING_REQUESTS: usize = 30;
const MAX_RETRY: usize = 5;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let bot = Bot::new(token);
    let actions = Actions::new(&bot, MAX_OUTSTANDING_REQUESTS, MAX_RETRY);
    let mut policy = PolicyState::new();
    let mut stream = Box::pin(polling_default(bot.clone()));
    info!("AhGroupBot started");
    while let Some(update) = stream.next().await {
        debug!("update: {:?}", update);
        if let Some((chat_id, msg_id)) = policy.get_message_to_delete(update?) {
            actions.spwan_delete_message(chat_id, msg_id).await;
        }
    }
    Ok(())
}
