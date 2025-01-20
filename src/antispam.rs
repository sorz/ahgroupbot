use std::{
    ops::{Add, AddAssign},
    sync::LazyLock,
};

use regex::Regex;
use sonic_rs::{Deserialize, Serialize};

static RE_SPAM_HIGH_RISK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(\d|黑|搬|送)(U|u)|开户|(会|會)(员|員)|收入|接入|",
        r"兼职|专职|咨询|日结|小白|钱|赚|支付|风险|主页|介绍|TRX|散户|",
        r"团队|专线|代理|合作|保底|日入|招人|商家|💵|💯|🧧|📣",
    ))
    .unwrap()
});

static RE_SPAM_MEDIUM_RISK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\d(W|w|K|k)|千|万|月|天|年|最|搞|做|操作|进群|做事|事情|了解|❤️|✈️",
    ))
    .unwrap()
});

static RE_SPAM_NO_RISK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(concat!(r"阿|啊|[aA]{3,}|[aA][hH]+",)).unwrap());

static RE_SPAM_FULL_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(concat!(r"🔥|看竹页",)).unwrap());

pub(crate) static SPAM_THREHOLD: u8 = 100;
static TEXT_SPAM_SCORE_MEDIUM_RISK: u8 = SPAM_THREHOLD / 2;
static TEXT_SPAM_SCORE_UNKNOWN_RISK: u8 = SPAM_THREHOLD / 6;

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

pub fn check_message_text(text: &str) -> SpamState {
    if RE_SPAM_NO_RISK.is_match(text) {
        SpamState::MaybeSpam(0)
    } else if RE_SPAM_HIGH_RISK.is_match(text) {
        SpamState::Spam
    } else if RE_SPAM_MEDIUM_RISK.is_match(text) {
        SpamState::MaybeSpam(TEXT_SPAM_SCORE_MEDIUM_RISK)
    } else {
        SpamState::MaybeSpam(TEXT_SPAM_SCORE_UNKNOWN_RISK)
    }
}

pub fn check_full_name_likely_spammer(name: &str) -> bool {
    RE_SPAM_FULL_NAME.is_match(name)
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

#[test]
fn test_spam_text() {
    let high = SpamState::Spam;
    let medium = SpamState::MaybeSpam(TEXT_SPAM_SCORE_MEDIUM_RISK);
    let unknown = SpamState::MaybeSpam(TEXT_SPAM_SCORE_UNKNOWN_RISK);
    let no_risk = SpamState::MaybeSpam(0);

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
    assert!(!check_full_name_likely_spammer("_(:з」∠)_"));
}
