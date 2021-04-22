# Ah Group Bot

A Telegram bot as group admin applying 啊 (ah) policy to the group.

Join [@AhAhAhGroup](https://t.me/AhAhAhGroup) to see how it works.

## The policy

The bot should be set as group admin so that it can delete any message that
violate the policy. Messages on private chat will be ignored.

- User can only post:
  - Plain text constituted with one or more 啊; or
  - A few allowed stickers
- No double posting
- No editing
- No rich text (incl. links, bold, etc.)
- No bot
- The number of 啊 on single post is at most it in the last post plus one
  - Except `啊`, `啊啊`, `啊啊啊`, and stickers, which can be posted at anytime
  - Allowed stickers is treat as single 啊

## Library used

- [teloxide](https://github.com/teloxide/teloxide): An elegant Telegram bots
  framework for Rust

[telegram-bot](https://github.com/telegram-rs/telegram-bot) was used but has
been replaced by teloxide due to lack of maintenance.
