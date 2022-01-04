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
- No links
- No bot
- The number of 啊 on single post is at most it in the last post plus one
  - Except `啊`, `啊啊`, `啊啊啊`, and stickers, which can be posted at anytime
  - Allowed stickers is treat as single 啊

## Anit-spammer rules

New member of a group should send at least one message containing 啊 or allowed
stickers in their first few messages, otherwise they will be banned by the bot.

Members already in the group before the bot join and members who has posted at
least one allowed message would never be banned regardless the number of
disallowed messages they sent.

## Configuration

Required environment variables:

- `TELEGRAM_BOT_TOKEN` - Telegram bot token

Optional environment variables:

- `STATE_DIRECTORY` - Where to store bot state, default to current working
  directory.
- `RUST_LOG` - Adjust log level, see
  [env_logger](https://rust-lang.github.io/log/env_logger/).

## Libraries used

- [teloxide](https://github.com/teloxide/teloxide): An elegant Telegram bots
  framework for Rust

[telegram-bot](https://github.com/telegram-rs/telegram-bot) was used but has
been replaced by teloxide due to lack of maintenance.

## See also

- HEX Counter
  ([Sodium-Aluminate/CountTo0xffffffff](https://github.com/Sodium-Aluminate/CountTo0xffffffff))
