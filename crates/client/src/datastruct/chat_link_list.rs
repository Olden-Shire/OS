// @ObfuscatedName("ce") — jag::oldscape::datastruct::ChatLinkList.
//
// Sibling of LinkList holding ChatLinkable entries. Java uses two
// separate LinkList families for chat history (one per channel) to
// avoid sharing the sentinel between the chat queues and the
// model-cache LRU.

#![allow(dead_code)]

use std::collections::VecDeque;

use super::chat_linkable::ChatLinkable;

pub struct ChatLinkList {
    pub entries: VecDeque<ChatLinkable>,
}

impl ChatLinkList {
    pub fn new() -> Self { Self { entries: VecDeque::new() } }
    pub fn push(&mut self, e: ChatLinkable) { self.entries.push_front(e); }
    pub fn pop_back(&mut self) -> Option<ChatLinkable> { self.entries.pop_back() }
    pub fn clear(&mut self) { self.entries.clear(); }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

impl Default for ChatLinkList { fn default() -> Self { Self::new() } }
