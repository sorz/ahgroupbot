use futures::StreamExt;
use lazy_static::lazy_static;
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    env,
};
use telegram_bot::*;
use tokio;

lazy_static! {
    static ref ALLOWED_STICKER_FILE_IDS: HashSet<&'static str> = {
        include_str!("stickers.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect()
    };
}

#[derive(Default)]
struct PolicyState {
    group_user_noa: HashMap<ChatId, (UserId, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ChatId {
    Group(GroupId),
    Supergroup(SupergroupId),
}

impl PolicyState {
    fn new() -> Self {
        Default::default()
    }

    fn is_message_allowed(&mut self, message: &Message) -> bool {
        let chat_id = match message.chat {
            MessageChat::Group(Group { id, .. }) => ChatId::Group(id),
            MessageChat::Supergroup(Supergroup { id, .. }) => ChatId::Supergroup(id),
            _ => return true, // Take action on groups only
        };
        let noa = match message.kind {
            MessageKind::Text {
                ref data,
                ref entities,
                ..
            } => {
                if !entities.is_empty() {
                    return false; // No links, formatting, etc.
                }
                if !data.chars().all(|c| c == '啊') {
                    return false; // 啊+ only
                }
                data.len() / 3
            }
            MessageKind::Sticker { ref data, .. } => {
                if ALLOWED_STICKER_FILE_IDS.contains(data.file_unique_id.as_str()) {
                    1 // Reset noa to 1
                } else {
                    return false; // Sticker not whitelisted
                }
            }
            MessageKind::NewChatTitle { .. }
            | MessageKind::NewChatPhoto { .. }
            | MessageKind::DeleteChatPhoto { .. }
            | MessageKind::MigrateToChatId { .. }
            | MessageKind::MigrateFromChatId { .. }
            | MessageKind::PinnedMessage { .. } => return true, // Allow them
            _ => return false, // Delete other messages
        };
        let uid = message.from.id;
        if message.reply_to_message.is_some() {
            return false; // No reply
        }
        match self.group_user_noa.entry(chat_id) {
            Entry::Vacant(entry) => {
                entry.insert((uid, noa));
                true // Allow any user & any noa if we lost tracking
            }
            Entry::Occupied(mut entry) => {
                let (last_uid, last_noa) = entry.get();
                if last_uid == &uid {
                    return false; // No single-user flooding
                }
                if noa > 3 && noa > last_noa + 1 {
                    return false; // No too many ah in a single message
                }
                entry.insert((uid, noa));
                true
            }
        }
    }

    fn get_message_to_delete(&mut self, update: Update) -> Option<Message> {
        match update.kind {
            UpdateKind::Message(message) if !self.is_message_allowed(&message) => Some(message),
            UpdateKind::EditedMessage(message) => Some(message),
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let api = Api::new(token);

    let mut policy = PolicyState::new();

    let mut stream = api.stream();
    while let Some(update) = stream.next().await {
        println!("update: {:?}", update);
        if let Some(message) = policy.get_message_to_delete(update?) {
            if let Err(err) = api.send(message.delete()).await {
                println!("Fail to delete: {:?}", err);
            }
        }
    }
    Ok(())
}
