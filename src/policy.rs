use log::{debug, info, warn};
use std::{borrow::Cow, convert::TryInto};
use teloxide::{
    Bot,
    dispatching::dialogue::GetChatId,
    prelude::Requester,
    types::{
        ChatId, ChatKind, ChatMemberKind, ChatMemberUpdated, Message, MessageEntityKind, MessageId,
        MessageKind, Sticker, Update, UpdateKind, UserId,
    },
};

use crate::{
    antispam::{SpamState, check_full_name_likely_spammer, check_message_text},
    storage::{AhCount, Storage},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Delete(ChatId, MessageId),
    Ban(ChatId, UserId),
    DeleteAndBan(ChatId, MessageId, UserId),
}

impl Action {
    pub fn get_delete(&self) -> Option<(ChatId, MessageId)> {
        match self {
            Self::Accept | Self::Ban(..) => None,
            Self::Delete(chat, msg) | Self::DeleteAndBan(chat, msg, _) => Some((*chat, *msg)),
        }
    }

    pub fn get_ban(&self) -> Option<(ChatId, UserId)> {
        match self {
            Self::Accept | Self::Delete(_, _) => None,
            Self::Ban(chat, user) => Some((*chat, *user)),
            Self::DeleteAndBan(chat, _, user) => Some((*chat, *user)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolicyState {
    bot: Bot,
    db: Storage,
    cid: ChatId,
}

impl PolicyState {
    pub async fn new(bot: Bot, db: Storage, cid: ChatId) -> anyhow::Result<Self> {
        Ok(Self { bot, db, cid })
    }

    pub async fn save(&mut self) -> anyhow::Result<()> {
        self.db.save().await
    }

    async fn get_sticker_set_title(&self, sticker: &Sticker) -> Option<String> {
        match self.bot.get_sticker_set(sticker.set_name.clone()?).await {
            Ok(set) => Some(set.title),
            Err(err) => {
                warn!("failed to get sticker set: {}", err);
                None
            }
        }
    }

    async fn check_message(&mut self, chat_id: ChatId, message: &Message) -> Action {
        let action_delete = Action::Delete(chat_id, message.id);
        match message.kind {
            // Allow some of system messages
            MessageKind::NewChatTitle(_)
            | MessageKind::NewChatPhoto(_)
            | MessageKind::DeleteChatPhoto(_)
            | MessageKind::Pinned(_) => return Action::Accept,
            // Check normal messages
            MessageKind::Common(_) => (),
            // Delete others
            _ => return action_delete,
        }
        let user = match &message.from {
            // No (other) bots
            Some(user) if user.is_bot => return action_delete,
            Some(user) => user,
            None => return Action::Accept,
        };

        // Check for spammer: message text, quoted text, and sticker name
        let text_to_check = [
            message.text().map(Cow::Borrowed),
            message
                .quote()
                .map(|quote| Cow::Borrowed(quote.text.as_str())),
            if let Some(sticker) = message.sticker() {
                self.get_sticker_set_title(sticker).await.map(Cow::Owned)
            } else {
                None
            },
        ];
        let spam_state = text_to_check
            .into_iter()
            .flatten()
            .map(check_message_text)
            .sum();
        let spam_state = self.db.update_user(&user.id, spam_state).await;
        if spam_state.is_spam() {
            return Action::DeleteAndBan(chat_id, message.id, user.id);
        }

        if message.reply_to_message().is_some() || message.quote().is_some() {
            return action_delete; // No reply or quote
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
                Some(sticker) => {
                    let file_id = sticker.file.unique_id.0.as_str();
                    if self.db.is_sticker_allowed(file_id).await {
                        // Treat allowed sticker as single 啊
                        1
                    } else if self
                        .bot
                        .get_chat_member(self.cid, user.id)
                        .await
                        .map(|member| member.is_privileged())
                        .unwrap_or_default()
                    {
                        // Allow admin to modify the allow list
                        info!("New sticker {file_id} added by {}", user.full_name());
                        self.db.add_allowed_sticker(file_id.to_string()).await;
                        1
                    } else {
                        // For other sitckers, delete
                        debug!("Sticker {file_id} is not in allowed list");
                        return action_delete;
                    }
                }
                // No text & no sticker?
                _ => return action_delete,
            },
            // 啊+ only
            Some(text) if !text.chars().all(|c| c == '啊') => return action_delete,
            // Each 啊 takes 3 bytes as UTF-8
            Some(text) => (text.len() / 3).try_into().expect("Toooooo mmmany ah"),
        };

        if let Err(err) = self.db.update_last_ah(AhCount::new(user.id, noa)).await {
            debug!("Reject message from [{}]: {}", user.id, err);
            return action_delete;
        }
        // Now they're a trusted user
        self.db.update_user(&user.id, SpamState::Authentic).await;
        Action::Accept
    }

    async fn check_member(&self, chat_id: ChatId, update: &ChatMemberUpdated) -> Action {
        let user = &update.new_chat_member.user;
        match &update.new_chat_member.kind {
            ChatMemberKind::Member(_) => {
                // Screen user name for spammer
                let fullname = user.full_name();
                info!("[{}] New user [{}]({}) join", chat_id, user.id, fullname);
                if check_full_name_likely_spammer(&fullname) {
                    info!("Ban user [{fullname}]({}) for their name", user.id);
                    Action::Ban(chat_id, user.id)
                } else if update.via_chat_folder_invite_link {
                    info!("Ban user [{fullname}]({}) via chat folder invite", user.id);
                    Action::Ban(chat_id, user.id)
                } else {
                    self.db.update_user(&user.id, SpamState::default()).await;
                    Action::Accept
                }
            }
            ChatMemberKind::Left => {
                info!("[{chat_id}] User [{}]({}) left", user.id, user.full_name());
                Action::Accept
            }
            ChatMemberKind::Banned(_) => {
                info!(
                    "[{chat_id}] User [{}]({}) banned by {}",
                    user.id,
                    user.full_name(),
                    update.from.full_name()
                );
                self.db.remove_user(&user.id).await;
                Action::Accept
            }
            _ => Action::Accept,
        }
    }

    pub async fn check_update(&mut self, update: &Update) -> Action {
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
        if chat.id != self.cid {
            info!("Ignore foreign chat {}", chat.id);
            return Action::Accept;
        }
        if let ChatKind::Public(_) = chat.kind {
            match update.kind {
                UpdateKind::ChatMember(ref update) => self.check_member(chat.id, update).await,
                UpdateKind::Message(ref msg) => self.check_message(chat.id, msg).await,
                UpdateKind::EditedMessage(ref msg) => Action::Delete(chat.id, msg.id),
                _ => Action::Accept,
            }
        } else {
            // Take action on groups only
            Action::Accept
        }
    }
}
