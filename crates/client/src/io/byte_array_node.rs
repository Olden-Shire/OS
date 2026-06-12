// @ObfuscatedName("ev") — jag::oldscape::io::ByteArrayNode.
//
// Linkable carrying a `byte[]` payload. Used by Java's friends-list /
// HTTPRequest worker queues to keep raw response bodies on a list
// without making them part of a parsed type.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct ByteArrayNode {
    // @ObfuscatedName("ev.r")
    pub data: Vec<u8>,
}

impl ByteArrayNode {
    pub fn new(data: Vec<u8>) -> Self { Self { data } }
}
