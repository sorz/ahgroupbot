use std::{
    collections::{HashMap, hash_map::Entry},
    path::Path,
    sync::Arc,
};

use anyhow::anyhow;
use sonic_rs::{Deserialize, Serialize};
use teloxide::types::{ChatId, UserId};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::Mutex,
};

use crate::antispam::SpamState;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Data {
    pub chats: HashMap<ChatId, (UserId, u32)>,
    pub users: HashMap<UserId, SpamState>,
}

#[derive(Debug, Clone)]
pub struct Storage {
    inner: Arc<Mutex<StorageImpl>>,
}

#[derive(Debug)]
struct StorageImpl {
    file: File,
    data: Data,
    buf: Vec<u8>,
}

impl StorageImpl {
    async fn save(&mut self) -> anyhow::Result<()> {
        self.buf.clear();
        sonic_rs::to_writer(&mut self.buf, &self.data)?;
        self.file.seek(SeekFrom::Start(0)).await?;
        self.file.write_all(&self.buf).await?;
        self.file.set_len(self.buf.len().try_into()?).await?;
        Ok(())
    }
}

impl Storage {
    pub async fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
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

        let inner = StorageImpl { file, data, buf };
        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    pub(crate) async fn save(&mut self) -> anyhow::Result<()> {
        self.inner.lock().await.save().await
    }

    pub(crate) async fn update_user(
        &mut self,
        user_id: &UserId,
        new_state: SpamState,
    ) -> SpamState {
        *self
            .inner
            .lock()
            .await
            .data
            .users
            .entry(*user_id)
            .and_modify(|e| *e += new_state)
            .or_insert(new_state)
    }

    pub(crate) async fn get_user(&self, user_id: &UserId) -> SpamState {
        self.inner
            .lock()
            .await
            .data
            .users
            .get(user_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) async fn get_chat(&self, chat_id: &ChatId) -> Option<(UserId, u32)> {
        self.inner.lock().await.data.chats.get(chat_id).cloned()
    }

    pub(crate) async fn update_chat(
        &mut self,
        chat_id: &ChatId,
        (user_id, noa): (UserId, u32),
    ) -> anyhow::Result<()> {
        match self.inner.lock().await.data.chats.entry(*chat_id) {
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

#[tokio::test]
async fn test_storage() {
    use crate::antispam::SPAM_THREHOLD;
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test.json");
    let mut storage = Storage::open(&path).await.unwrap();

    // Chat ops
    storage
        .update_chat(&ChatId(1), (UserId(1), 10))
        .await
        .unwrap();
    storage
        .update_chat(&ChatId(1), (UserId(2), 5))
        .await
        .unwrap();
    storage
        .update_chat(&ChatId(1), (UserId(1), 6))
        .await
        .unwrap();
    storage
        .update_chat(&ChatId(1), (UserId(2), 1))
        .await
        .unwrap();
    storage
        .update_chat(&ChatId(1), (UserId(1), 3))
        .await
        .unwrap();
    storage
        .update_chat(&ChatId(1), (UserId(2), 3))
        .await
        .unwrap();
    assert!(
        storage
            .update_chat(&ChatId(1), (UserId(1), 5))
            .await
            .is_err()
    );
    assert!(
        storage
            .update_chat(&ChatId(1), (UserId(2), 4))
            .await
            .is_err()
    );

    // Spam state ops
    assert_eq!(
        storage.update_user(&UserId(1), SpamState::Spam).await,
        SpamState::Spam
    );
    assert_eq!(
        storage.update_user(&UserId(1), SpamState::Authentic).await,
        SpamState::Authentic
    );
    assert_eq!(
        storage
            .update_user(&UserId(2), SpamState::with_score(10))
            .await,
        SpamState::with_score(10)
    );
    assert_eq!(
        storage
            .update_user(&UserId(2), SpamState::with_score(20))
            .await,
        SpamState::with_score(30)
    );
    assert_eq!(
        storage
            .update_user(&UserId(2), SpamState::with_score(SPAM_THREHOLD - 10))
            .await,
        SpamState::Spam
    );
    assert_eq!(
        storage
            .update_user(&UserId(2), SpamState::with_score(1))
            .await,
        SpamState::Spam
    );
    storage
        .update_user(&UserId(3), SpamState::with_score(20))
        .await;
    storage.save().await.unwrap();
    storage.save().await.unwrap(); // redundancy

    let storage = Storage::open(&path).await.unwrap();
    assert_eq!(storage.get_user(&UserId(1)).await, SpamState::Authentic);
    assert_eq!(storage.get_user(&UserId(2)).await, SpamState::Spam);
    assert_eq!(
        storage.get_user(&UserId(3)).await,
        SpamState::with_score(20)
    );
    assert_eq!(storage.get_user(&UserId(4)).await, SpamState::with_score(0));

    assert!(!storage.get_user(&UserId(1)).await.is_spam());
    assert!(storage.get_user(&UserId(2)).await.is_spam());
    assert!(!storage.get_user(&UserId(3)).await.is_spam());
}
