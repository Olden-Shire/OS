// jagex3.datastruct package
//
// Intrusive doubly-linked lists + hashtables. Java uses raw next/prev
// references with sentinel nodes; we mirror that with raw pointers boxed
// behind a clean API. Unsafe is confined to this module.

pub mod chat_linkable;
pub mod chat_link_list;
pub mod linkable;
pub mod linkable2;
pub mod link_list;
pub mod link_list2;
pub mod hash_table;
pub mod int_hash_table;
pub mod lru_cache;
