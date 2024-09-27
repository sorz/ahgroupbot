use std::{
    collections::{hash_map::Entry, HashMap},
    ops::{Add, AddAssign},
    path::Path,
};

use anyhow::anyhow;
use sonic_rs::{Deserialize, Serialize};
use teloxide::types::{ChatId, UserId};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
};

pub(crate) static SPAM_THREHOLD: u8 = 100;

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub enum SpamState {
    Authentic,
    MaybeSpam(u8),
    Spam,
}

impl Default for SpamState {
    fn default() -> Self {
        Self::MaybeSpam(0)
    }
}

impl Add for SpamState {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Authentic, _) | (_, Self::Authentic) => Self::Authentic,
            (Self::Spam, _) | (_, Self::Spam) => Self::Spam,
            (Self::MaybeSpam(a), Self::MaybeSpam(b)) => {
                if a + b < SPAM_THREHOLD {
                    Self::MaybeSpam(a + b)
                } else {
                    Self::Spam
                }
            }
        }
    }
}

impl AddAssign for SpamState {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SpamState {
    pub(crate) fn is_spam(&self) -> bool {
        matches!(self, Self::Spam)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Data {
    pub chats: HashMap<ChatId, (UserId, u32)>,
    pub users: HashMap<UserId, SpamState>,
}

#[derive(Debug)]
pub(crate) struct Storage {
    file: File,
    data: Data,
    buf: Vec<u8>,
}

impl Storage {
    pub(crate) async fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let mut file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .await?;

        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await?;
        let data: Data = if buf.is_empty() {
            Default::default()
        } else {
            sonic_rs::from_slice(&buf)?
        };

        Ok(Self { file, data, buf })
    }

    pub(crate) async fn save(&mut self) -> anyhow::Result<()> {
        self.buf.clear();
        sonic_rs::to_writer(&mut self.buf, &self.data)?;
        self.file.seek(SeekFrom::Start(0)).await?;
        self.file.write_all(&self.buf).await?;
        self.file.set_len(self.buf.len().try_into()?).await?;
        Ok(())
    }

    pub(crate) fn update_user(&mut self, user_id: &UserId, new_state: SpamState) -> SpamState {
        *self
            .data
            .users
            .entry(*user_id)
            .and_modify(|e| *e += new_state)
            .or_insert(new_state)
    }

    pub(crate) fn get_user(&self, user_id: &UserId) -> SpamState {
        self.data.users.get(user_id).cloned().unwrap_or_default()
    }

    pub(crate) fn get_chat(&self, chat_id: &ChatId) -> Option<(UserId, u32)> {
        self.data.chats.get(chat_id).cloned()
    }

    pub(crate) fn update_chat(
        &mut self,
        chat_id: &ChatId,
        (user_id, noa): (UserId, u32),
    ) -> anyhow::Result<()> {
        match self.data.chats.entry(*chat_id) {
            Entry::Occupied(mut e) => {
                if e.get().0 == user_id {
                    Err(anyhow!("No single-user flooding"))
                } else if noa > 3 && noa > e.get().1 + 1 {
                    Err(anyhow!("No too many ah in a single message"))
                } else {
                    e.insert((user_id, noa));
                    Ok(())
                }
            }
            Entry::Vacant(e) => {
                // For group w/o history, anyone & any noa is allowed
                e.insert((user_id, noa));
                Ok(())
            }
        }
    }
}

#[test]
fn test_spam_state_ops() {
    // Authentic take highest priority
    assert_eq!(
        SpamState::Authentic,
        SpamState::Authentic + SpamState::Authentic
    );
    assert_eq!(SpamState::Authentic, SpamState::Authentic + SpamState::Spam);
    assert_eq!(SpamState::Authentic, SpamState::Spam + SpamState::Authentic);
    assert_eq!(
        SpamState::Authentic,
        SpamState::MaybeSpam(0) + SpamState::Authentic
    );

    // MaybeSpam ops
    assert_eq!(
        SpamState::MaybeSpam(3),
        SpamState::MaybeSpam(1) + SpamState::MaybeSpam(2)
    );
    assert_eq!(
        SpamState::Spam,
        SpamState::MaybeSpam(1) + SpamState::MaybeSpam(SPAM_THREHOLD - 1)
    );
    assert_eq!(SpamState::Spam, SpamState::MaybeSpam(1) + SpamState::Spam);
    assert_eq!(SpamState::Spam, SpamState::Spam + SpamState::MaybeSpam(1));
}

#[tokio::test]
async fn test_storage() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test.json");
    let mut storage = Storage::open(&path).await.unwrap();

    // Chat ops
    storage.update_chat(&ChatId(1), (UserId(1), 10)).unwrap();
    storage.update_chat(&ChatId(1), (UserId(2), 5)).unwrap();
    storage.update_chat(&ChatId(1), (UserId(1), 6)).unwrap();
    storage.update_chat(&ChatId(1), (UserId(2), 1)).unwrap();
    storage.update_chat(&ChatId(1), (UserId(1), 3)).unwrap();
    storage.update_chat(&ChatId(1), (UserId(2), 3)).unwrap();
    assert!(storage.update_chat(&ChatId(1), (UserId(1), 5)).is_err());
    assert!(storage.update_chat(&ChatId(1), (UserId(2), 4)).is_err());

    // Spam state ops
    assert_eq!(
        storage.update_user(&UserId(1), SpamState::Spam),
        SpamState::Spam
    );
    assert_eq!(
        storage.update_user(&UserId(1), SpamState::Authentic),
        SpamState::Authentic
    );
    assert_eq!(
        storage.update_user(&UserId(2), SpamState::MaybeSpam(10)),
        SpamState::MaybeSpam(10)
    );
    assert_eq!(
        storage.update_user(&UserId(2), SpamState::MaybeSpam(20)),
        SpamState::MaybeSpam(30)
    );
    assert_eq!(
        storage.update_user(&UserId(2), SpamState::MaybeSpam(SPAM_THREHOLD - 10)),
        SpamState::Spam
    );
    assert_eq!(
        storage.update_user(&UserId(2), SpamState::MaybeSpam(1)),
        SpamState::Spam
    );
    storage.update_user(&UserId(3), SpamState::MaybeSpam(20));
    storage.save().await.unwrap();
    storage.save().await.unwrap(); // redundancy

    let storage = Storage::open(&path).await.unwrap();
    assert_eq!(storage.get_user(&UserId(1)), SpamState::Authentic);
    assert_eq!(storage.get_user(&UserId(2)), SpamState::Spam);
    assert_eq!(storage.get_user(&UserId(3)), SpamState::MaybeSpam(20));
    assert_eq!(storage.get_user(&UserId(4)), SpamState::MaybeSpam(0));

    assert!(!storage.get_user(&UserId(1)).is_spam());
    assert!(storage.get_user(&UserId(2)).is_spam());
    assert!(!storage.get_user(&UserId(3)).is_spam());
}
