use ahgroupbot::{Actions, PolicyState};
use futures::StreamExt;
use log::{debug, info, warn};
use std::{env, fs, path::PathBuf, time::Duration};
use teloxide::{
    dispatching::update_listeners::{polling_default, AsUpdateStream},
    Bot, RequestError,
};
use tokio::time::sleep;

// Avoid unlimited concurrent requests sending to Telegram server.
// Not sure if it is necessary, set as a safeguard anyway.
const MAX_OUTSTANDING_REQUESTS: usize = 30;

const MAX_RETRY: u32 = 5;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);
const COUNT_TO_BAN: u32 = 4;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let mut token_path: PathBuf = env::var("CREDENTIALS_DIRECTORY")
        .unwrap_or("./".into())
        .into();
    token_path.push("token");
    let token = fs::read_to_string(&token_path).map_err(|e| {
        eprintln!(
            "fail to read token from $CREDENTIALS_DIRECTORY/token `{}`",
            token_path.display()
        );
        e
    })?;

    let mut db_path = env::var("STATE_DIRECTORY")
        .map(|p| p.into())
        .or_else(|_| env::current_dir())
        .expect("STATE_DIRECTORY not a valid path");
    db_path.push("state.sled-db");

    let bot = Bot::new(token.trim());
    let actions = Actions::new(&bot, MAX_OUTSTANDING_REQUESTS, MAX_RETRY);
    let mut policy =
        PolicyState::new(&db_path, COUNT_TO_BAN).expect("Failed to open/create policy state file");
    let mut poll = polling_default(bot.clone()).await;
    let mut stream = Box::pin(poll.as_stream());
    let mut retry_count = 0u32;
    info!("AhGroupBot started");
    while let Some(update) = stream.next().await {
        debug!("Update: {:?}", update);
        let update = match update {
            Ok(update) => {
                retry_count = 0;
                update
            }
            Err(RequestError::Network(err)) if retry_count < MAX_RETRY => {
                warn!("Netwrok error: {}", err);
                sleep(RETRY_BASE_DELAY * 2u32.pow(retry_count)).await;
                retry_count += 1;
                continue;
            }
            Err(err) => return Err(err.into()),
        };
        if let Some((chat_id, msg_id)) = policy.get_message_to_delete(&update) {
            actions.spwan_delete_message(chat_id, msg_id).await;
        }
        if let Some((chat_id, user_id)) = policy.get_user_to_ban(&update) {
            actions.spawn_ban_user(chat_id, user_id).await;
        }
    }
    Ok(())
}
