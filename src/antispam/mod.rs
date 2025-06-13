pub(crate) mod background;

use std::{
    cmp,
    iter::Sum,
    ops::{Add, AddAssign},
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

use regex::Regex;
use sonic_rs::{Deserialize, Serialize};

static RE_SPAM_HIGH_RISK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(\d|黑|搬|送|)(U|u)|开户|(会|會)(员|員)|收入|接入|免费|完整版|",
        r"兼职|专职|咨询|日结|小白|钱|赚|支付|风险|主页|介绍|TRX|散户|",
        r"母狗|轮流|内射|\d\d岁|学妹|初中|高中|大学|金主|爸爸|老公|白眼|",
        r"团队|专线|代理|合作|保底|日入|商家|红包|盘口|急需|吋|侑|莳|玖|",
        r"(预|預)(付|服)|搬砖|玳|代付|点位|(滴|嘀)(窝|我)|群演|助手|",
        r"做工|招人|捡漏|项目|视频|",
        r"💵|💯|🧧|📣|➡️|⬅️|👉|👈",
    ))
    .unwrap()
});

static RE_SPAM_MEDIUM_RISK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\d(W|w|K|k)|千|万|月|天|年|最|搞|做|操作|进群|做事|事情|了解|",
        r"打字|联系|[1-5]00|押|抢|领|招|美丽|冲|来|兄弟|爽|",
        r"❤️|✈️|🤝|😍"
    ))
    .unwrap()
});

static RE_SPAM_NO_RISK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"阿|啊|[aA]{3,}|[aA][hH]+").unwrap());

static RE_SPAM_FULL_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"🔥|看(主|竹)页|会(员|員)|赚钱|达利|^dali|来(了|咯)|[\u206a-\u206f]").unwrap()
});

pub(crate) static SPAM_THREHOLD: u8 = 100;
static TEXT_SPAM_SCORE_MEDIUM_RISK: u8 = SPAM_THREHOLD / 2;
static TEXT_SPAM_SCORE_UNKNOWN_RISK: u8 = SPAM_THREHOLD / 6;

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub enum SpamState {
    Authentic,
    MaybeSpam {
        score: u8,
        create_ts_secs: u64,
        update_ts_secs: u64,
    },
}

fn now_ts_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_secs()
}

impl Default for SpamState {
    fn default() -> Self {
        let now = now_ts_secs();
        Self::MaybeSpam {
            score: 0,
            create_ts_secs: now,
            update_ts_secs: now,
        }
    }
}

impl Add for SpamState {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Authentic, _) | (_, Self::Authentic) => Self::Authentic,
            (
                Self::MaybeSpam {
                    score: score1,
                    create_ts_secs: cs1,
                    update_ts_secs: us1,
                },
                Self::MaybeSpam {
                    score: score2,
                    create_ts_secs: cs2,
                    update_ts_secs: us2,
                },
            ) => Self::MaybeSpam {
                score: score1.saturating_add(score2),
                create_ts_secs: cmp::min(cs1, cs2),
                update_ts_secs: cmp::max(us1, us2),
            },
        }
    }
}

impl AddAssign for SpamState {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sum for SpamState {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |a, b| a + b)
    }
}

impl SpamState {
    pub(crate) fn is_spam(&self) -> bool {
        matches!(self, Self::MaybeSpam { score, .. } if *score >= SPAM_THREHOLD)
    }

    pub(crate) fn is_authentic(&self) -> bool {
        matches!(self, Self::Authentic)
    }

    pub(crate) fn with_score(score: u8) -> Self {
        let now = now_ts_secs();
        Self::MaybeSpam {
            score,
            create_ts_secs: now,
            update_ts_secs: now,
        }
    }

    pub(crate) fn new_spam() -> Self {
        Self::with_score(SPAM_THREHOLD.saturating_add(1))
    }
}

pub fn check_message_text<T: AsRef<str>>(text: T) -> SpamState {
    if RE_SPAM_NO_RISK.is_match(text.as_ref()) {
        SpamState::with_score(0)
    } else if RE_SPAM_HIGH_RISK.is_match(text.as_ref()) {
        SpamState::new_spam()
    } else if RE_SPAM_MEDIUM_RISK.is_match(text.as_ref()) {
        SpamState::with_score(TEXT_SPAM_SCORE_MEDIUM_RISK)
    } else {
        SpamState::with_score(TEXT_SPAM_SCORE_UNKNOWN_RISK)
    }
}

pub fn check_full_name_likely_spammer(name: &str) -> bool {
    if name.contains('|') || name.contains('｜') {
        false
    } else {
        RE_SPAM_FULL_NAME.is_match(name)
    }
}

#[test]
fn test_spam_state_ops() {
    // Authentic take highest priority
    assert_eq!(
        SpamState::Authentic,
        SpamState::Authentic + SpamState::Authentic
    );
    assert_eq!(
        SpamState::Authentic,
        SpamState::Authentic + SpamState::new_spam()
    );
    assert_eq!(
        SpamState::Authentic,
        SpamState::new_spam() + SpamState::Authentic
    );
    assert_eq!(
        SpamState::Authentic,
        SpamState::with_score(0) + SpamState::Authentic
    );

    // MaybeSpam ops
    assert_eq!(
        SpamState::with_score(3),
        SpamState::with_score(1) + SpamState::with_score(2)
    );
    assert!((SpamState::with_score(1) + SpamState::with_score(SPAM_THREHOLD - 1)).is_spam());
    assert!((SpamState::with_score(1) + SpamState::new_spam()).is_spam());
    assert!((SpamState::new_spam() + SpamState::with_score(1)).is_spam());
}

#[test]
fn test_spam_timestamp_ops() {
    let old = SpamState::MaybeSpam {
        score: 0,
        create_ts_secs: 100,
        update_ts_secs: 100,
    };
    let new = SpamState::MaybeSpam {
        score: 1,
        create_ts_secs: 200,
        update_ts_secs: 200,
    };
    let updated = SpamState::MaybeSpam {
        score: 1,
        create_ts_secs: 100,
        update_ts_secs: 200,
    };
    assert_eq!(old + new, updated);
    assert_eq!(new + old, updated);
}

#[test]
fn test_spam_text() {
    let high = SpamState::new_spam();
    let medium = SpamState::with_score(TEXT_SPAM_SCORE_MEDIUM_RISK);
    let unknown = SpamState::with_score(TEXT_SPAM_SCORE_UNKNOWN_RISK);
    let no_risk = SpamState::with_score(0);

    assert_eq!(no_risk, check_message_text("aaa"));
    assert_eq!(no_risk, check_message_text("test[AAa]test"));
    assert_eq!(no_risk, check_message_text("AHh!!"));
    assert_eq!(no_risk, check_message_text("啊啊"));
    assert_eq!(no_risk, check_message_text("开户啊5k")); // be conservative
    assert_eq!(unknown, check_message_text(""));
    assert_eq!(unknown, check_message_text("123"));
    assert_eq!(medium, check_message_text("5k"));
    assert_eq!(medium, check_message_text("…搞事情…"));
    assert_eq!(high, check_message_text("…搬U…"));
    assert_eq!(high, check_message_text("…3天开户…"));
}

#[test]
fn test_spam_name() {
    assert!(check_full_name_likely_spammer("立即来🔥赚麻了"));
    assert!(check_full_name_likely_spammer("来看竹页吧"));
    assert!(check_full_name_likely_spammer("legacy\u{206e}codepint"));
    assert!(!check_full_name_likely_spammer("_(:з」∠)_"));
    assert!(!check_full_name_likely_spammer("啊啊|赚钱"));
}
