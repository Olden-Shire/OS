// @ObfuscatedName("cn")
//
// jagex3.datastruct.IntHashTable — open-addressed (key, value) table baked
// from a fixed key array at construction. Used by Js5 to map group/file
// name hashes onto group/file ids.

#![allow(dead_code)]

pub struct IntHashTable {
    // @ObfuscatedName("cn.r")
    pub buckets: Vec<i32>,
}

impl IntHashTable {
    pub fn new(keys: &[i32]) -> Self {
        let mut bucket_count = 1i32;
        while bucket_count <= (keys.len() as i32 >> 1) + keys.len() as i32 {
            bucket_count <<= 1;
        }

        let total = (bucket_count + bucket_count) as usize;
        let mut buckets = vec![-1i32; total];

        let mask = bucket_count - 1;
        for value in 0..keys.len() {
            let mut hash = keys[value] & mask;
            while buckets[(hash + hash + 1) as usize] != -1 {
                hash = (hash + 1) & mask;
            }
            buckets[(hash + hash) as usize] = keys[value];
            buckets[(hash + hash + 1) as usize] = value as i32;
        }

        Self { buckets }
    }

    // @ObfuscatedName("cn.r(I)I")
    pub fn find(&self, key: i32) -> i32 {
        let mask = ((self.buckets.len() >> 1) - 1) as i32;
        let mut hash = key & mask;
        loop {
            let value = self.buckets[(hash + hash + 1) as usize];
            if value == -1 {
                return -1;
            }
            if self.buckets[(hash + hash) as usize] == key {
                return value;
            }
            hash = (hash + 1) & mask;
        }
    }
}
