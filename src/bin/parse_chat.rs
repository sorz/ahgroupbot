//! Initilize bot status from Telegram-client-exported chat history JSONs
//!
//! ./parse_chat <group-1.json> [group-2.json ...] > status.json
use anyhow::{anyhow, bail};
use regex::Regex;
use sonic_rs::{Deserialize, FastStr, Serialize};
use std::{
    env,
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
    sync::LazyLock,
};
use teloxide::types::{ChatId, UserId};

use ahgroupbot::{AhCount, SpamState, StorageData};

static RE_USER_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(user|channel)(\d+)$").unwrap());

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ChatHistory {
    id: ChatId,
    messages: Vec<Message>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
enum Message {
    Service,
    Message { text: Text, from_id: FastStr },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
enum Text {
    Plain(FastStr),
    Formatted(Vec<TextSegment>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
enum TextSegment {
    Plain(FastStr),
    Formatted { text: FastStr },
}

fn count_noa(text: &str) -> anyhow::Result<u32> {
    if text.chars().any(|c| c != '啊') {
        bail!("Non-ah charactar found");
    }
    Ok((text.len() / 3).try_into()?)
}

impl Text {
    fn count_noa(&self) -> anyhow::Result<u32> {
        match self {
            Self::Plain(text) => count_noa(text),
            Self::Formatted(segments) => segments
                .iter()
                .try_fold(0u32, |noa, seg| count_noa(seg.text()).map(|n| noa + n)),
        }
    }
}

impl TextSegment {
    fn text(&self) -> &FastStr {
        match self {
            Self::Plain(text) => text,
            Self::Formatted { text } => text,
        }
    }
}

impl Message {
    fn parse_user_noa(&self) -> anyhow::Result<AhCount> {
        match self {
            Self::Service => bail!("service message, not a user text message"),
            Self::Message { text, from_id } => {
                let noa = text.count_noa()?;
                let captures = RE_USER_ID
                    .captures(from_id)
                    .ok_or_else(|| anyhow!("from_id ({}) not match regex", from_id))?;
                let id = captures.get(2).unwrap().as_str();
                Ok(AhCount::new(UserId(id.parse()?), noa))
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let paths: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        bail!("No input JSON file provided on CLI argument");
    }

    let mut output_state: StorageData = Default::default();
    let mut buf = vec![0u8; 0];
    for path in paths {
        eprintln!("Parsing chat history {:?}", path);
        buf.clear();
        let mut last_ah: Option<AhCount> = None;
        File::open(&path)?.read_to_end(&mut buf)?;
        let history: ChatHistory = sonic_rs::from_slice(&buf)?;
        for (msg_id, msg) in history.messages.iter().enumerate() {
            match msg.parse_user_noa() {
                Ok(ah) => {
                    output_state.users.insert(ah.uid, SpamState::Authentic);
                    last_ah = Some(ah);
                }
                Err(err) => {
                    eprintln!("Msg#{:06} - ignored: {}", msg_id, err);
                }
            }
        }
        if let Some(ah) = last_ah {
            output_state.last_ah = Some(ah);
        }
    }
    // TODO: use to_writer_pretty after sonic_rs v0.4 released
    let buf = sonic_rs::to_vec_pretty(&output_state)?;
    io::stdout().write_all(&buf)?;
    Ok(())
}
