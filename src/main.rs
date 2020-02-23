use futures::StreamExt;
use std::collections::{hash_map::Entry, HashMap};
use std::env;
use telegram_bot::*;
use tokio;

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

    fn is_accept(&mut self, message: &Message) -> bool {
        let chat_id = match message.chat {
            MessageChat::Group(Group { id, .. }) => ChatId::Group(id),
            MessageChat::Supergroup(Supergroup { id, .. }) => ChatId::Supergroup(id),
            _ => return true, // Take action on groups only
        };
        let noa = match message.kind {
            MessageKind::Text { ref data, .. } => {
                if !data.chars().all(|c| c == '啊') {
                    return false; // 啊+ only
                }
                data.len() / 3
            }
            MessageKind::Audio { .. }
            | MessageKind::Document { .. }
            | MessageKind::Photo { .. }
            | MessageKind::Sticker { .. }
            | MessageKind::Contact { .. }
            | MessageKind::Location { .. } => return false,
            _ => return true, // Ingore other messages
        };
        let uid = message.from.id;
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
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let api = Api::new(token);

    let mut policy = PolicyState::new();

    let mut stream = api.stream();
    while let Some(update) = stream.next().await {
        // println!("update: {:?}", update);
        if let UpdateKind::Message(message) = update?.kind {
            if !policy.is_accept(&message) {
                api.send(message.delete()).await?;
            }
        }
    }
    Ok(())
}
