use crate::{ChatId, MessageId, UserId};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};
use lazy_static::lazy_static;
use log::{info, warn};
use std::{collections::HashSet, convert::TryInto, fmt::Write, path::Path};
use teloxide::types::{ChatKind, ChatMemberKind, Message, MessageKind, Update, UpdateKind, User};

lazy_static! {
    static ref ALLOWED_STICKER_FILE_IDS: HashSet<&'static str> = {
        include_str!("stickers.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect()
    };
}

#[derive(Debug)]
pub struct PolicyState {
    db: sled::Db,
    count_to_ban: u32,
}

impl PolicyState {
    pub fn new<P: AsRef<Path>>(db_path: P, count_to_ban: u32) -> Result<Self, sled::Error> {
        Ok(Self {
            db: sled::open(db_path)?,
            count_to_ban,
        })
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
        // Count the number of ah (noa)
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
            Some(text) => (text.len() / 3).try_into().expect("Toooooo mmmany ah"),
        };

        if let Some((last_uid, last_noa)) = self.get_user_noa(chat_id) {
            if last_uid == uid {
                return false; // No single-user flooding
            }
            if noa > 3 && noa > last_noa + 1 {
                return false; // No too many ah in a single message
            }
        } // For group w/o history, anyone & any noa is allowed

        self.put_user_noa(chat_id, uid, noa);
        self.stop_trace_user(chat_id, uid); // Now a trusted user
        true
    }

    fn is_message_likely_spam<'a>(
        &mut self,
        chat_id: ChatId,
        message: &'a Message,
    ) -> Option<&'a User> {
        match (&message.kind, message.from()) {
            (MessageKind::Common(_), Some(sender)) => {
                if let Some(sticker) = message.sticker() {
                    if !ALLOWED_STICKER_FILE_IDS.contains(&*sticker.file_unique_id) {
                        Some(sender)
                    } else {
                        None
                    }
                } else if let Some(text) = message.text() {
                    if !text.contains('啊') {
                        info!(
                            "[{}] Potential spam [{}]({}): {}",
                            chat_id,
                            sender.id,
                            sender.full_name(),
                            text
                        );
                        Some(sender)
                    } else {
                        None
                    }
                } else {
                    Some(sender)
                }
            }
            _ => None, // Ignore messages other than common message
        }
    }

    pub fn get_message_to_delete(&mut self, update: &Update) -> Option<(ChatId, MessageId)> {
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

    pub fn get_user_to_ban(&mut self, update: &Update) -> Option<(ChatId, UserId)> {
        let chat = update.chat()?;
        if let ChatKind::Public(_) = chat.kind {
            match update.kind {
                UpdateKind::Message(ref msg) => {
                    if let Some(spammer) = self.is_message_likely_spam(chat.id, msg) {
                        if self.should_ban_user(chat.id, spammer.id) {
                            return Some((chat.id, spammer.id));
                        }
                    }
                }
                UpdateKind::ChatMember(ref msg) => match msg.new_chat_member.kind {
                    ChatMemberKind::Member => {
                        info!(
                            "[{}] New user [{}]({}) join",
                            chat.id,
                            msg.from.id,
                            msg.from.full_name(),
                        );
                        self.new_user_join(chat.id, msg.from.id);
                    }
                    ChatMemberKind::Left | ChatMemberKind::Banned(_) => {
                        info!(
                            "[{}] User [{}]({}) left",
                            chat.id,
                            msg.from.id,
                            msg.from.full_name(),
                        );
                        self.stop_trace_user(chat.id, msg.from.id);
                    }
                    _ => (),
                },
                _ => (),
            }
        }
        None
    }

    fn get_user_noa(&self, cid: ChatId) -> Option<(UserId, u32)> {
        let mut key = format!("uid-{}", cid);
        let uid = self
            .db
            .get(&key)
            .expect("Error during read policy state (uid)")
            .map(|bytes| (&*bytes).read_i64::<LE>());

        key.clear();
        write!(&mut key, "noa-{}", cid).unwrap();
        let noa = self
            .db
            .get(&key)
            .expect("Error during read policy state (noa)")
            .map(|bytes| (&*bytes).read_u32::<LE>());

        match (uid, noa) {
            (Some(Ok(uid)), Some(Ok(noa))) => Some((uid, noa)),
            (None, _) | (_, None) => None,
            (Some(Err(err)), _) | (_, Some(Err(err))) => {
                warn!("Broken data on policy state (uid and/or noa): {}", err);
                None
            }
        }
    }

    fn put_user_noa(&self, cid: ChatId, uid: UserId, noa: u32) {
        let mut key = format!("uid-{}", cid);
        let mut buf = vec![];
        buf.write_i64::<LE>(uid).unwrap();
        self.db
            .insert(&key, &*buf)
            .expect("Error during write policy state (uid)");

        key.clear();
        buf.clear();
        write!(&mut key, "noa-{}", cid).unwrap();
        buf.write_u32::<LE>(noa).unwrap();
        self.db
            .insert(&key, &*buf)
            .expect("Error during write policy state (noa)");
    }

    fn new_user_join(&self, cid: ChatId, uid: UserId) {
        let key = format!("untrust-uid-{}-{}", cid, uid);
        let mut buf = vec![];
        buf.write_u32::<LE>(0).unwrap();
        self.db
            .insert(&key, &*buf)
            .expect("Error during write policy state (unstrust uid)");
    }

    fn stop_trace_user(&self, cid: ChatId, uid: UserId) {
        let key = format!("untrust-uid-{}-{}", cid, uid);
        self.db
            .remove(key)
            .expect("Error during write policy state (remove untrust uid)");
    }

    pub fn should_ban_user(&self, cid: ChatId, uid: UserId) -> bool {
        let key = format!("untrust-uid-{}-{}", cid, uid);
        let buf = self
            .db
            .update_and_fetch(key, |v| {
                if let Some(mut v) = v {
                    let mut n = v.read_u32::<LE>().unwrap();
                    n += 1;
                    Some(n.to_le_bytes().to_vec())
                } else {
                    None
                }
            })
            .expect("Error during update policy state (untrust uid)");
        if let Some(buf) = buf {
            buf.as_ref()
                .read_u32::<LE>()
                .expect("Error during read polify state")
                >= self.count_to_ban
        } else {
            false
        }
    }
}
