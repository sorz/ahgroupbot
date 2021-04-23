use crate::{ChatId, MessageId, UserId};
use lazy_static::lazy_static;
use std::collections::{hash_map::Entry, HashMap, HashSet};
use teloxide::types::{ChatKind, Message, MessageKind, Update, UpdateKind};

lazy_static! {
    static ref ALLOWED_STICKER_FILE_IDS: HashSet<&'static str> = {
        include_str!("stickers.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect()
    };
}

#[derive(Default)]
pub struct PolicyState {
    group_user_noa: HashMap<ChatId, (UserId, usize)>,
}

impl PolicyState {
    pub fn new() -> Self {
        Default::default()
    }

    fn is_message_allowed(&mut self, chat_id: ChatId, message: &Message) -> bool {
        match message.kind {
            // Allow some of system messages
            MessageKind::NewChatTitle(_)
            | MessageKind::NewChatPhoto(_)
            | MessageKind::DeleteChatPhoto(_)
            | MessageKind::Migrate(_)
            | MessageKind::Pinned(_) => return true,
            // Check normal messages
            MessageKind::Common(_) => (),
            // Delete others
            _ => return false,
        }
        let uid = match message.from() {
            // No (other) bots
            Some(user) if user.is_bot => return false,
            Some(user) => user.id,
            None => return false,
        };
        if message.reply_to_message().is_some() {
            return false; // No reply
        }
        if !message.entities().unwrap_or(&[]).is_empty() {
            return false; // No links, formatting, etc.
        }
        let noa = match message.text() {
            None => match message.sticker() {
                // Treat allowed sticker as single 啊
                Some(sticker) if ALLOWED_STICKER_FILE_IDS.contains(&*sticker.file_unique_id) => 1,
                // No neither-text-or-allowed-sticker messages
                _ => return false,
            },
            // 啊+ only
            Some(text) if !text.chars().all(|c| c == '啊') => return false,
            // Each 啊 takes 3 bytes as UTF-8
            Some(text) => text.len() / 3,
        };
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

    pub fn get_message_to_delete(&mut self, update: Update) -> Option<(ChatId, MessageId)> {
        let chat = update.chat()?;
        if let ChatKind::Public(_) = chat.kind {
            match update.kind {
                UpdateKind::Message(ref msg) if !self.is_message_allowed(chat.id, msg) => {
                    Some((chat.id, msg.id))
                }
                UpdateKind::EditedMessage(ref msg) => Some((chat.id, msg.id)),
                _ => None,
            }
        } else {
            // Take action on groups only
            None
        }
    }
}