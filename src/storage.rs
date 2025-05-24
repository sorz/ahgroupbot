use std::{
    collections::{HashMap, hash_map},
    path::Path,
    sync::Arc,
};

use anyhow::anyhow;
use sonic_rs::{Deserialize, Serialize};
use teloxide::types::UserId;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::Mutex,
};

use crate::antispam::SpamState;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AhCount {
    pub uid: UserId,
    pub noa: u32, // Number of "ah"
}

impl AhCount {
    pub fn new(uid: UserId, noa: u32) -> Self {
        Self { uid, noa }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Data {
    pub last_ah: Option<AhCount>,
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

    pub(crate) async fn update_user(&self, user_id: &UserId, new_state: SpamState) -> SpamState {
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

    pub(crate) async fn remove_user(&self, user_id: &UserId) {
        self.inner.lock().await.data.users.remove(user_id);
    }

    pub(crate) async fn update_last_ah(&self, new_ah: AhCount) -> anyhow::Result<()> {
        match self.inner.lock().await.data.last_ah {
            Some(ref mut last_ah) => {
                if last_ah.uid == new_ah.uid {
                    Err(anyhow!("No single-user flooding"))
                } else if new_ah.noa > 3 && new_ah.noa > last_ah.noa + 1 {
                    Err(anyhow!("No too many ah in a single message"))
                } else {
                    *last_ah = new_ah;
                    Ok(())
                }
            }
            ref mut last_ah @ None => {
                // If no history, anyone & any noa is allowed
                *last_ah = Some(new_ah);
                Ok(())
            }
        }
    }

    pub(crate) async fn with_user_states<F, R>(&self, f: F) -> R
    where
        F: FnOnce(hash_map::Iter<UserId, SpamState>) -> R,
    {
        let inner = self.inner.lock().await;
        let iter = inner.data.users.iter();
        f(iter)
    }
}

#[tokio::test]
async fn test_storage() {
    use crate::antispam::SPAM_THREHOLD;
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("test.json");
    let mut storage = Storage::open(&path).await.unwrap();

    // Ah count
    storage
        .update_last_ah(AhCount::new(UserId(1), 10))
        .await
        .unwrap();
    storage
        .update_last_ah(AhCount::new(UserId(2), 5))
        .await
        .unwrap();
    storage
        .update_last_ah(AhCount::new(UserId(1), 6))
        .await
        .unwrap();
    storage
        .update_last_ah(AhCount::new(UserId(2), 1))
        .await
        .unwrap();
    storage
        .update_last_ah(AhCount::new(UserId(1), 3))
        .await
        .unwrap();
    storage
        .update_last_ah(AhCount::new(UserId(2), 3))
        .await
        .unwrap();
    assert!(
        storage
            .update_last_ah(AhCount::new(UserId(1), 5))
            .await
            .is_err()
    );
    assert!(
        storage
            .update_last_ah(AhCount::new(UserId(2), 4))
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
