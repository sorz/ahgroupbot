use log::{debug, info};
use std::{collections::HashSet, convert::TryInto, path::Path, sync::LazyLock};
use teloxide::{
    dispatching::dialogue::GetChatId,
    types::{
        ChatId, ChatKind, Message, MessageEntityKind, MessageId, MessageKind, Update, UpdateKind,
        UserId,
    },
};

use crate::{
    antispam::is_text_match_spam,
    storage::{self, SpamState, Storage},
};

static ALLOWED_STICKER_FILE_IDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    include_str!("stickers.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect()
});

static SPAM_SCORE_LOW: u8 = storage::SPAM_THREHOLD / 6;
static SPAM_SCORE_HIGH: u8 = storage::SPAM_THREHOLD / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Delete(ChatId, MessageId),
    DeleteAndBan(ChatId, MessageId, UserId),
}

impl Action {
    pub fn get_delete(&self) -> Option<(ChatId, MessageId)> {
        match self {
            Self::Accept => None,
            Self::Delete(chat, msg) | Self::DeleteAndBan(chat, msg, _) => Some((*chat, *msg)),
        }
    }

    pub fn get_ban(&self) -> Option<(ChatId, UserId)> {
        match self {
            Self::Accept | Self::Delete(_, _) => None,
            Self::DeleteAndBan(chat, _, user) => Some((*chat, *user)),
        }
    }
}

#[derive(Debug)]
pub struct PolicyState {
    db: Storage,
}

impl PolicyState {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> anyhow::Result<Self> {
        Ok(Self {
            db: Storage::open(db_path).await?,
        })
    }

    pub async fn save(&mut self) -> anyhow::Result<()> {
        self.db.save().await
    }

    fn check_message(&mut self, chat_id: ChatId, message: &Message) -> Action {
        let action_delete = Action::Delete(chat_id, message.id);
        match message.kind {
            // Allow some of system messages
            MessageKind::NewChatTitle(_)
            | MessageKind::NewChatPhoto(_)
            | MessageKind::DeleteChatPhoto(_)
            | MessageKind::Pinned(_) => return Action::Accept,
            // Screen new user for spammer
            MessageKind::NewChatMembers(ref members) => {
                for member in &members.new_chat_members {
                    let fullname = member.full_name();
                    info!(
                        "[{}] New user [{}]({}) join",
                        message.chat.id, member.id, fullname,
                    );
                    if fullname.contains('🔥') {
                        // Fast path to ban
                        info!("Ban user [{}] with fire emoji", fullname);
                        return Action::DeleteAndBan(chat_id, message.id, member.id);
                    }
                }
            }
            // Check normal messages
            MessageKind::Common(_) => (),
            // Delete others
            _ => return action_delete,
        }
        let uid = match &message.from {
            // No (other) bots
            Some(user) if user.is_bot => return action_delete,
            Some(user) => user.id,
            None => return Action::Accept,
        };

        // Check for spammer
        if let Some(text) = message.text() {
            if !text.contains("啊") {
                let state = SpamState::MaybeSpam(if is_text_match_spam(text) {
                    info!("Spam (high score) [{}]: {}", uid, text);
                    SPAM_SCORE_HIGH
                } else {
                    info!("Spam (low score) [{}]: {}", uid, text);
                    SPAM_SCORE_LOW
                });
                let state = self.db.update_user(&uid, state);
                if state.is_spam() {
                    return Action::DeleteAndBan(chat_id, message.id, uid);
                }
            }
        }

        if message.reply_to_message().is_some() {
            return action_delete; // No reply
        }
        if message.entities().unwrap_or(&[]).iter().any(|entity| {
            !matches!(
                entity.kind,
                MessageEntityKind::Bold
                    | MessageEntityKind::Underline
                    | MessageEntityKind::Italic
                    | MessageEntityKind::Code
                    | MessageEntityKind::Strikethrough
                    | MessageEntityKind::Spoiler
            )
        }) {
            // Whitelist stylish text but no clickable things like URL, mention, etc.
            return action_delete;
        }
        // Count the number of ah (noa)
        let noa = match message.text() {
            None => match message.sticker() {
                // Treat allowed sticker as single 啊
                Some(sticker) if ALLOWED_STICKER_FILE_IDS.contains(&*sticker.file.unique_id) => 1,
                // No neither-text-or-allowed-sticker messages
                _ => return action_delete,
            },
            // 啊+ only
            Some(text) if !text.chars().all(|c| c == '啊') => return action_delete,
            // Each 啊 takes 3 bytes as UTF-8
            Some(text) => (text.len() / 3).try_into().expect("Toooooo mmmany ah"),
        };

        if let Err(err) = self.db.update_chat(&chat_id, (uid, noa)) {
            debug!("Reject message from [{}]: {}", uid, err);
            return action_delete;
        }
        // Now they're a trusted user
        self.db.update_user(&uid, storage::SpamState::Authentic);
        Action::Accept
    }

    pub fn check_update(&mut self, update: &Update) -> Action {
        if let UpdateKind::Error(value) = &update.kind {
            info!(
                "Unsupported update [{:?}/{}]: {}",
                update.chat_id(),
                update.id.0,
                value
            );
            // TODO: try to extract message id from `value`
            // return Some((update.chat_id()?, MessageId(update.id)));
            return Action::Accept;
        }
        let chat = match update.chat() {
            Some(chat) => chat,
            None => return Action::Accept,
        };
        if let ChatKind::Public(_) = chat.kind {
            match update.kind {
                UpdateKind::Message(ref msg) => self.check_message(chat.id, msg),
                UpdateKind::EditedMessage(ref msg) => Action::Delete(chat.id, msg.id),
                _ => Action::Accept,
            }
        } else {
            // Take action on groups only
            Action::Accept
        }
    }
}
