// jag::oldscape::friends — friend list, ignore list, private chat
// filter, friend-chat (clan) roster.

#![allow(dead_code)]

use crate::datastruct::chat_linkable::ChatLinkable;

// @ObfuscatedName("ec") — jag::oldscape::client::FriendListEntry.
#[derive(Debug, Clone, Default)]
pub struct FriendListEntry {
    // @ObfuscatedName("ec.r") — current player display name.
    pub name: String,
    // @ObfuscatedName("ec.d") — previous display name (rendered when
    // the friend has changed name since last sighting).
    pub previous_name: String,
    // @ObfuscatedName("ec.l") — 0 = offline, else 1..N world id.
    pub world_id: i32,
    // @ObfuscatedName("ec.m") — clan rank within the user's clan.
    pub rank: i32,
    // @ObfuscatedName("ec.c") — invite referrer marker.
    pub referrer: i32,
    // @ObfuscatedName("ec.n") — true if the friend referred us.
    pub referred: bool,
}

// @ObfuscatedName("ev") — jag::oldscape::client::IgnoreListEntry.
#[derive(Debug, Clone, Default)]
pub struct IgnoreListEntry {
    // @ObfuscatedName("ev.r")
    pub name: String,
    // @ObfuscatedName("ev.d")
    pub display_name: String,
}

// @ObfuscatedName("dx") — jag::oldscape::friends::FriendChatUser.
//
// One member of the active friend-chat (clan) channel. Java extends
// Linkable so the entries hang off a LinkList; we hold them on a Vec.
#[derive(Debug, Clone, Default)]
pub struct FriendChatUser {
    pub username: String,
    pub display_name: String,
    pub world: i32,
    pub rank: i32,
}

// @ObfuscatedName("bb") — jagex3.friends.PrivateChatFilter.
//
// Java assigns the `index` field 0→ON, 1→FRIENDS, 2→OFF (the standard OSRS
// private-chat encoding). The cs2 chat-options interface reads
// chat_getfilter_private (= index) to highlight the current setting, so these
// indices MUST match Java exactly — earlier this enum had On/Off swapped
// (Off=0/On=2), which left the round-tripped send path working but made the
// getter report the wrong filter to scripts.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateChatFilter {
    On = 0,
    Friends = 1,
    Off = 2,
}

impl Default for PrivateChatFilter {
    fn default() -> Self { Self::On }
}

impl PrivateChatFilter {
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => Self::On,
            1 => Self::Friends,
            2 => Self::Off,
            // Java PrivateChatFilter.get() returns null for out-of-range;
            // chat_setfilter then falls back to FRIENDS.
            _ => Self::Friends,
        }
    }

    // Java's PrivateChatFilter.index — the enum's `index` field.
    pub fn index(self) -> i32 { self as i32 }
}

// @ObfuscatedName("dh") — jag::oldscape::friends::TimestampMessage.
//
// Extends ChatLinkable (the chat-history sibling of Linkable). Used by
// the rate-limit / dedupe path for incoming chat — the timestamp lets
// the receiver skip repeats sent within a short window.
#[derive(Debug, Clone)]
pub struct TimestampMessage {
    pub base: ChatLinkable,
    pub message: String,
    pub world_id: i32,
    pub timestamp_ms: i64,
}

impl TimestampMessage {
    pub fn new(id: i64, message: String, world_id: i32) -> Self {
        Self {
            base: ChatLinkable::new(id),
            message,
            world_id,
            timestamp_ms: crate::util::monotonic_time::current_time(),
        }
    }
}
