// @ObfuscatedName("co") — jag::oldscape::datastruct::ChatLinkable.
//
// Sibling of Linkable kept on a separate `ChatLinkList` so the chat
// history can be iterated independently of the LinkList2 used by other
// systems. Java models this as an intrusive doubly-linked list node
// with `next` and `prev` raw references and an `unlink` method.
//
// Rust can't model raw intrusive pointers cleanly outside `unsafe`, so
// we mirror the Java field shape (id + next_id + prev_id) and rely on
// the owning `ChatLinkList` to walk by id. The TimestampMessage
// subtype embeds this struct as its first field so the same node ops
// apply.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct ChatLinkable {
    // @ObfuscatedName("co.r") — primary key (Java's `long id`).
    pub id: i64,
    // @ObfuscatedName("co.d") / "co.l" — `next` / `prev` ids the
    // owning list uses to chain entries. A value of `i64::MIN`
    // indicates "no neighbour" (sentinel slot).
    pub next_id: i64,
    pub prev_id: i64,
    // @ObfuscatedName("co.m") — `unlinked` flag set by `unlink()` so
    // the list can free the slot lazily during the next iteration.
    pub unlinked: bool,
}

const SENTINEL: i64 = i64::MIN;

impl ChatLinkable {
    pub fn new(id: i64) -> Self {
        Self { id, next_id: SENTINEL, prev_id: SENTINEL, unlinked: false }
    }

    // @ObfuscatedName("co.r()V") — ChatLinkable.unlink. Marks the node
    // for removal; the next list walk drops it after rewiring its
    // neighbours.
    pub fn unlink(&mut self) {
        self.unlinked = true;
    }

    pub fn has_next(&self) -> bool { self.next_id != SENTINEL }
    pub fn has_prev(&self) -> bool { self.prev_id != SENTINEL }
}
