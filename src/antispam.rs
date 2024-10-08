use std::sync::LazyLock;

use regex::Regex;

static RE_SPAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"((\d|黑|搬)(U|u)|\d(W|w)|千|万|月|天|年|开户|最|(会|會)(员|員)|收入|接入|了解|",
        r"做事|事情|兼职|专职|咨询|日结|小白|钱|搞|做|赚|支付|风险|主页|进群|介绍|TRX|散户|",
        r"团队|专线|代理|合作|保底|日入|招人|💵|💯|🧧|📣|❤️)",
    ))
    .unwrap()
});

pub fn is_text_match_spam(text: &str) -> bool {
    RE_SPAM.is_match(text)
}
