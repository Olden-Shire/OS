// @ObfuscatedName("cu")
//
// jagex3.js5.Js5Net — JS5 download orchestrator. Holds the active socket,
// the urgent/prefetch queues, and the incoming-group state machine.
// All state is `static` in Java; we hold it in a single Mutex<Js5NetState>
// so the per-field @ObfuscatedName annotations still apply.

#![allow(dead_code)]

use std::sync::{LazyLock, Mutex};

use crc32fast::Hasher;

use crate::datastruct::hash_table::HashTable;
use crate::datastruct::link_list2::LinkList2;
use crate::game_shell::monotonic_ms;
use crate::io::client_stream::{ClientStream, IoError};
use crate::io::packet::Packet;

use super::js5_loader::Js5Loader;
use super::js5_net_request::Js5NetRequest;

// custom registry — Java refers to Js5Loader instances directly; in the
// port we hold them by slot id so callers can dispatch without a borrow
// through the Js5Net mutex.
pub static LOADERS: LazyLock<Mutex<Vec<Option<Box<Js5Loader>>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

pub fn register_loader(mut loader: Box<Js5Loader>) -> i32 {
    let mut reg = LOADERS.lock().unwrap();
    let slot = reg.len() as i32;
    loader.slot = slot;
    reg.push(Some(loader));
    slot
}

pub struct Js5NetState {
    // @ObfuscatedName("g.r") — ClientStream stream
    pub stream: Option<ClientStream>,

    // @ObfuscatedName("cu.d")
    pub timeout_ms: i32,

    // @ObfuscatedName("bo.l")
    pub last_tick_ms: i64,

    // @ObfuscatedName("cu.m")
    pub pending_urgent_queue: HashTable,

    // @ObfuscatedName("cu.c")
    pub pending_urgent_queue_size: i32,

    // @ObfuscatedName("cu.n")
    pub urgent_queue: HashTable,

    // @ObfuscatedName("cu.j")
    pub urgent_queue_size: i32,

    // @ObfuscatedName("cu.z")
    pub request_queue: LinkList2,

    // @ObfuscatedName("cu.g")
    pub pending_prefetch_queue: HashTable,

    // @ObfuscatedName("cu.q")
    pub pending_prefetch_queue_size: i32,

    // @ObfuscatedName("cu.i")
    pub prefetch_queue: HashTable,

    // @ObfuscatedName("cu.s")
    pub prefetch_queue_size: i32,

    // @ObfuscatedName("cu.u")
    pub incoming_urgent_request: bool,

    // @ObfuscatedName("bx.v")
    pub incoming_request: *mut Js5NetRequest,

    // @ObfuscatedName("cu.w")
    pub incoming_transfer_header: Packet,

    // @ObfuscatedName("cu.e")
    pub incoming_group_buffer: Option<Packet>,

    // @ObfuscatedName("cu.b")
    pub incoming_chunk_pos: i32,

    // @ObfuscatedName("ab.f")
    pub master_index_buffer: Option<Packet>,

    // @ObfuscatedName("cu.k") — Js5Loader[256], indexed by archive id; we
    // mirror that with slot ids that resolve via LOADERS.
    pub field1200: [i32; 256],

    // @ObfuscatedName("cu.o")
    pub xor_key: i8,

    // @ObfuscatedName("cu.a")
    pub crc_error_count: i32,

    // @ObfuscatedName("cu.h")
    pub io_error_count: i32,

    // custom — backing storage for requests so we can hand out *mut to
    // HashTable / LinkList2 without violating ownership. The pointer
    // remains valid for the lifetime of the request.
    pub request_arena: Vec<Box<Js5NetRequest>>,
}

impl Js5NetState {
    fn new() -> Self {
        Self {
            stream: None,
            timeout_ms: 0,
            last_tick_ms: 0,
            pending_urgent_queue: HashTable::new(4096),
            pending_urgent_queue_size: 0,
            urgent_queue: HashTable::new(32),
            urgent_queue_size: 0,
            request_queue: LinkList2::new(),
            pending_prefetch_queue: HashTable::new(4096),
            pending_prefetch_queue_size: 0,
            prefetch_queue: HashTable::new(4096),
            prefetch_queue_size: 0,
            incoming_urgent_request: false,
            incoming_request: std::ptr::null_mut(),
            incoming_transfer_header: Packet::with_size(8),
            incoming_group_buffer: None,
            incoming_chunk_pos: 0,
            master_index_buffer: None,
            field1200: [-1; 256],
            xor_key: 0,
            crc_error_count: 0,
            io_error_count: 0,
            request_arena: Vec::new(),
        }
    }

    fn alloc_request(&mut self) -> *mut Js5NetRequest {
        let boxed = Box::new(Js5NetRequest::new());
        let ptr: *mut Js5NetRequest = Box::into_raw(boxed);
        // Re-box and keep alive in the arena so the address stays stable.
        // SAFETY: we'll keep this Box for the lifetime of the state.
        let owned = unsafe { Box::from_raw(ptr) };
        self.request_arena.push(owned);
        ptr
    }
}

unsafe impl Send for Js5NetState {}

pub static STATE: LazyLock<Mutex<Js5NetState>> = LazyLock::new(|| Mutex::new(Js5NetState::new()));

// custom — helper that lifts a `*mut Linkable` (the hashtable's bucket
// pointer type) back to the Js5NetRequest it lives inside. Safe so long
// as the original bucket entry came from `Js5NetState::alloc_request`.
unsafe fn req_from_linkable(p: *mut crate::datastruct::linkable::Linkable) -> *mut Js5NetRequest {
    // Linkable is the first field of Linkable2, which is the first field
    // (`base.base`) of Js5NetRequest. Hence offset 0.
    p as *mut Js5NetRequest
}

unsafe fn req_from_linkable2(p: *mut crate::datastruct::linkable2::Linkable2) -> *mut Js5NetRequest {
    p as *mut Js5NetRequest
}

unsafe fn linkable_of(r: *mut Js5NetRequest) -> *mut crate::datastruct::linkable::Linkable {
    // base.base
    unsafe { &mut (*r).base.base as *mut _ }
}

unsafe fn linkable2_of(r: *mut Js5NetRequest) -> *mut crate::datastruct::linkable2::Linkable2 {
    unsafe { &mut (*r).base as *mut _ }
}

// @ObfuscatedName("by.r(B)Z")
pub fn loop_tick() -> bool {
    let mut s = STATE.lock().unwrap();

    let current_time_ms = monotonic_ms();
    let mut time_delta = (current_time_ms - s.last_tick_ms) as i32;
    s.last_tick_ms = current_time_ms;
    if time_delta > 200 {
        time_delta = 200;
    }
    s.timeout_ms += time_delta;

    if s.prefetch_queue_size == 0
        && s.urgent_queue_size == 0
        && s.pending_prefetch_queue_size == 0
        && s.pending_urgent_queue_size == 0
    {
        return true;
    }

    if s.stream.is_none() {
        return false;
    }

    if s.timeout_ms > 30000 {
        return on_io_error(&mut s);
    }

    // Drain pending_urgent into urgent (up to 20 in flight).
    while s.urgent_queue_size < 20 && s.pending_urgent_queue_size > 0 {
        let p = s.pending_urgent_queue.search();
        if p.is_null() {
            break;
        }
        let req = unsafe { req_from_linkable(p) };
        let key = unsafe { (*linkable_of(req)).key };
        let mut packet = Packet::with_size(4);
        packet.p1(1);
        packet.p3(key as i32);
        eprintln!("[js5] -> urgent req key=0x{key:x} (archive={}, group={})", key >> 16, key & 0xffff);
        if let Some(stream) = s.stream.as_mut() {
            if stream.write(&packet.data, 0, 4).is_err() {
                return on_io_error(&mut s);
            }
        }
        unsafe { s.urgent_queue.put(linkable_of(req), key) };
        s.pending_urgent_queue_size -= 1;
        s.urgent_queue_size += 1;
    }

    while s.prefetch_queue_size < 20 && s.pending_prefetch_queue_size > 0 {
        let p2 = s.request_queue.next();
        if p2.is_null() {
            break;
        }
        let req = unsafe { req_from_linkable2(p2) };
        let key = unsafe { (*linkable_of(req)).key };
        let mut packet = Packet::with_size(4);
        packet.p1(0);
        packet.p3(key as i32);
        if let Some(stream) = s.stream.as_mut() {
            if stream.write(&packet.data, 0, 4).is_err() {
                return on_io_error(&mut s);
            }
        }
        unsafe { (*linkable2_of(req)).unlink2() };
        unsafe { s.prefetch_queue.put(linkable_of(req), key) };
        s.pending_prefetch_queue_size -= 1;
        s.prefetch_queue_size += 1;
    }

    for _read_iter in 0..100 {
        let available_bytes = match s.stream.as_mut().unwrap().available() {
            Ok(n) => n,
            Err(_) => return on_io_error(&mut s),
        };
        if available_bytes < 0 {
            return on_io_error(&mut s);
        }
        if available_bytes == 0 {
            break;
        }
        if _read_iter == 0 {
            eprintln!("[js5] <- {available_bytes} bytes available");
        }
        s.timeout_ms = 0;

        let mut header_size: i32 = 0;
        if s.incoming_request.is_null() {
            header_size = 8;
        } else if s.incoming_chunk_pos == 0 {
            header_size = 1;
        }

        if header_size > 0 {
            let mut readable = header_size - s.incoming_transfer_header.pos;
            if readable > available_bytes {
                readable = available_bytes;
            }
            let pos = s.incoming_transfer_header.pos as i32;
            let xor_key = s.xor_key;
            let data_ptr = s.incoming_transfer_header.data.as_mut_ptr();
            let result = {
                let stream = s.stream.as_mut().unwrap();
                let slice = unsafe { std::slice::from_raw_parts_mut(data_ptr.add(pos as usize), readable as usize) };
                stream.read(slice, 0, readable)
            };
            if result.is_err() {
                return on_io_error(&mut s);
            }
            if xor_key != 0 {
                let p = s.incoming_transfer_header.pos as usize;
                for j in 0..readable as usize {
                    s.incoming_transfer_header.data[p + j] ^= xor_key as u8;
                }
            }
            s.incoming_transfer_header.pos += readable;
            if s.incoming_transfer_header.pos < header_size {
                break;
            }

            if s.incoming_request.is_null() {
                let dump: Vec<String> = s.incoming_transfer_header.data[..8].iter().map(|b| format!("{b:02x}")).collect();
                eprintln!("[js5] header bytes = [{}]", dump.join(" "));
                s.incoming_transfer_header.pos = 0;
                let archive_id = s.incoming_transfer_header.g1();
                let group_id = s.incoming_transfer_header.g2();
                let compression_type = s.incoming_transfer_header.g1();
                let compressed_size = s.incoming_transfer_header.g4();
                let key = ((archive_id as i64) << 16) + group_id as i64;
                eprintln!("[js5] <- header archive={archive_id} group={group_id} ctype={compression_type} clen={compressed_size} key=0x{key:x}");
                let request_ptr = s.urgent_queue.find(key);
                s.incoming_urgent_request = true;
                let request_ptr = if request_ptr.is_null() {
                    s.incoming_urgent_request = false;
                    s.prefetch_queue.find(key)
                } else {
                    request_ptr
                };
                if request_ptr.is_null() {
                    eprintln!("[js5] header for key=0x{key:x} not in queues — IO ERROR");
                    return on_io_error(&mut s);
                }
                let req = unsafe { req_from_linkable(request_ptr) };
                let group_header_size = if compression_type == 0 { 5 } else { 9 };
                s.incoming_request = req;
                let padding = unsafe { (*req).padding } as i32;
                let total = compressed_size + group_header_size + padding;
                let mut group_buf = Packet::with_size(total);
                group_buf.p1(compression_type);
                group_buf.p4(compressed_size);
                s.incoming_group_buffer = Some(group_buf);
                s.incoming_chunk_pos = 8;
                s.incoming_transfer_header.pos = 0;
            } else if s.incoming_chunk_pos == 0 {
                let marker_byte = s.incoming_transfer_header.data[0];
                let key = unsafe { (*linkable_of(s.incoming_request)).key };
                if key == 0xff000f {
                    eprintln!("[js5][a15] marker read = 0x{marker_byte:02x}");
                }
                if marker_byte as i8 == -1 {
                    s.incoming_chunk_pos = 1;
                    s.incoming_transfer_header.pos = 0;
                } else {
                    eprintln!("[js5] MARKER MISMATCH key=0x{key:x} byte=0x{marker_byte:02x} chunk_pos was reset to 0 but byte wasn't 0xff");
                    s.incoming_request = std::ptr::null_mut();
                }
            }
        } else {
            let padding = unsafe { (*s.incoming_request).padding } as i32;
            let xor_key = s.xor_key;
            let incoming_chunk_pos = s.incoming_chunk_pos;
            let (pos, data_ptr, buf_len) = {
                let buf = s.incoming_group_buffer.as_mut().unwrap();
                (buf.pos as usize, buf.data.as_mut_ptr(), buf.data.len() as i32)
            };
            let remaining_bytes = buf_len - padding;

            let mut chunk_remaining = 512 - incoming_chunk_pos;
            if chunk_remaining > remaining_bytes - pos as i32 {
                chunk_remaining = remaining_bytes - pos as i32;
            }
            if chunk_remaining > available_bytes {
                chunk_remaining = available_bytes;
            }

            let result = {
                let stream = s.stream.as_mut().unwrap();
                let slice = unsafe { std::slice::from_raw_parts_mut(data_ptr.add(pos), chunk_remaining as usize) };
                stream.read(slice, 0, chunk_remaining)
            };
            if result.is_err() {
                return on_io_error(&mut s);
            }
            let new_buf_pos = {
                let buf = s.incoming_group_buffer.as_mut().unwrap();
                if xor_key != 0 {
                    for j in 0..chunk_remaining as usize {
                        buf.data[pos + j] ^= xor_key as u8;
                    }
                }
                buf.pos += chunk_remaining;
                buf.pos
            };
            s.incoming_chunk_pos += chunk_remaining;

            let cur_key = unsafe { (*linkable_of(s.incoming_request)).key };
            if cur_key == 0xff000f {
                let buf = s.incoming_group_buffer.as_ref().unwrap();
                let end = (new_buf_pos as usize).min(buf.data.len());
                let start = end.saturating_sub(20);
                let dump: Vec<String> = buf.data[start..end].iter().map(|b| format!("{b:02x}")).collect();
                eprintln!("[js5][a15] chunk_remaining={} pos={} new_buf_pos={} chunk_pos={} remaining={} last20=[{}]", chunk_remaining, pos, new_buf_pos, s.incoming_chunk_pos, remaining_bytes, dump.join(" "));
            }
            if new_buf_pos == remaining_bytes {
                let key = unsafe { (*linkable_of(s.incoming_request)).key };
                if key == 0xff00ff {
                    let group_buf = s.incoming_group_buffer.take().unwrap();
                    s.master_index_buffer = Some(group_buf);
                    // Each registered loader pulls its (crc, version) tuple
                    // out of the master index payload now.
                    let field = s.field1200;
                    drop(s);
                    {
                        let mut s2 = STATE.lock().unwrap();
                        let mib = s2.master_index_buffer.as_mut().unwrap() as *mut Packet;
                        drop(s2);
                        let mut reg = LOADERS.lock().unwrap();
                        for j in 0..256 {
                            let slot = field[j];
                            if slot < 0 {
                                continue;
                            }
                            unsafe {
                                (*mib).pos = (j * 8 + 5) as i32;
                                let index_crc = (*mib).g4();
                                let index_version = (*mib).g4();
                                if let Some(loader) = reg.get_mut(slot as usize).and_then(|o| o.as_mut()) {
                                    loader.request_index(index_crc, index_version);
                                }
                            }
                        }
                    }
                    s = STATE.lock().unwrap();
                } else {
                    let buf_ref = s.incoming_group_buffer.as_ref().unwrap();
                    let remaining = remaining_bytes as usize;
                    let mut hasher = Hasher::new();
                    hasher.update(&buf_ref.data[..remaining]);
                    let crc = hasher.finalize() as i32;
                    let expected = unsafe { (*s.incoming_request).expected_crc };
                    if expected != crc {
                        eprintln!("[js5] CRC MISMATCH key=0x{key:x} expected=0x{expected:08x} got=0x{crc:08x} remaining={remaining}");
                        if let Some(stream) = s.stream.as_mut() {
                            stream.close();
                        }
                        s.crc_error_count += 1;
                        s.stream = None;
                        s.xor_key = (random_byte() as i8).max(1);
                        return false;
                    }
                    eprintln!("[js5] group complete key=0x{key:x} size={remaining}");
                    s.crc_error_count = 0;
                    s.io_error_count = 0;
                    let req = s.incoming_request;
                    let provider_slot = unsafe { (*req).provider };
                    let key = unsafe { (*linkable_of(req)).key };
                    let group_data = s.incoming_group_buffer.take().unwrap().data;
                    let is_index = (key & 0xFF0000) == 0xFF0000;
                    let is_urgent = s.incoming_urgent_request;
                    drop(s);
                    let mut reg = LOADERS.lock().unwrap();
                    if let Some(loader) = reg.get_mut(provider_slot as usize).and_then(|o| o.as_mut()) {
                        loader.write((key & 0xFFFF) as i32, group_data, is_index, is_urgent);
                    }
                    drop(reg);
                    s = STATE.lock().unwrap();
                }

                // unlink + decrement queue counters
                let req = s.incoming_request;
                unsafe { (*linkable_of(req)).unlink() };
                if s.incoming_urgent_request {
                    s.urgent_queue_size -= 1;
                } else {
                    s.prefetch_queue_size -= 1;
                }
                s.incoming_chunk_pos = 0;
                s.incoming_request = std::ptr::null_mut();
            } else if s.incoming_chunk_pos != 512 {
                break;
            } else {
                s.incoming_chunk_pos = 0;
            }
        }
    }
    true
}

fn on_io_error(s: &mut Js5NetState) -> bool {
    eprintln!("[js5] IO ERROR (count {} → {})", s.io_error_count, s.io_error_count + 1);
    if let Some(stream) = s.stream.as_mut() {
        stream.close();
    }
    s.io_error_count += 1;
    s.stream = None;
    false
}

fn random_byte() -> u8 {
    // Mirrors `(byte) (Math.random() * 255.0D + 1.0D)`. Replace with the
    // gamepack's PRNG when Random is ported; std::random isn't stable yet
    // so we use the monotonic clock as a tiny shift source.
    let n = (monotonic_ms() as u64).wrapping_mul(2862933555777941757) >> 8;
    ((n & 0xFF) as u8).saturating_add(1)
}

// @ObfuscatedName("p.d(ZI)V")
pub fn send_login_logout_packet(logged_in: bool) {
    let mut s = STATE.lock().unwrap();
    if s.stream.is_none() {
        return;
    }
    let mut packet = Packet::with_size(4);
    packet.p1(if logged_in { 2 } else { 3 });
    packet.p3(0);
    if let Some(stream) = s.stream.as_mut() {
        if stream.write(&packet.data, 0, 4).is_err() {
            stream.close();
            s.io_error_count += 1;
            s.stream = None;
        }
    }
}

// @ObfuscatedName("q.l(Lam;ZB)V")
pub fn init(stream: ClientStream, logged_in: bool) {
    {
        let mut s = STATE.lock().unwrap();
        if let Some(old) = s.stream.as_mut() {
            old.close();
        }
        s.stream = Some(stream);
    }
    send_login_logout_packet(logged_in);
    let mut s = STATE.lock().unwrap();
    s.incoming_transfer_header.pos = 0;
    s.incoming_request = std::ptr::null_mut();
    s.incoming_group_buffer = None;
    s.incoming_chunk_pos = 0;

    // Move urgent → pending_urgent.
    loop {
        let p = s.urgent_queue.search();
        if p.is_null() {
            break;
        }
        let key = unsafe { (*p).key };
        unsafe { s.pending_urgent_queue.put(p, key) };
        s.pending_urgent_queue_size += 1;
        s.urgent_queue_size -= 1;
    }
    // Move prefetch → pending_prefetch via request_queue.
    loop {
        let p = s.prefetch_queue.search();
        if p.is_null() {
            break;
        }
        let key = unsafe { (*p).key };
        let req = unsafe { req_from_linkable(p) };
        unsafe { s.request_queue.push_front(linkable2_of(req)) };
        unsafe { s.pending_prefetch_queue.put(p, key) };
        s.pending_prefetch_queue_size += 1;
        s.prefetch_queue_size -= 1;
    }

    if s.xor_key != 0 {
        let mut packet = Packet::with_size(4);
        packet.p1(4);
        packet.p1(s.xor_key as i32);
        packet.p2(0);
        if let Some(stream) = s.stream.as_mut() {
            if stream.write(&packet.data, 0, 4).is_err() {
                stream.close();
                s.io_error_count += 1;
                s.stream = None;
            }
        }
    }
    s.timeout_ms = 0;
    s.last_tick_ms = monotonic_ms();
}

// @ObfuscatedName("by.m(Ldq;IIIBZI)V")
pub fn queue_request(
    provider: i32,
    archive_id: i32,
    group_id: i32,
    expected_crc: i32,
    padding: i8,
    urgent: bool,
) {
    let mut s = STATE.lock().unwrap();
    let key = ((archive_id as i64) << 16) + group_id as i64;
    if !s.pending_urgent_queue.find(key).is_null() {
        return;
    }
    if !s.urgent_queue.find(key).is_null() {
        return;
    }
    let pending_prefetch = s.pending_prefetch_queue.find(key);
    if pending_prefetch.is_null() {
        if !urgent && !s.prefetch_queue.find(key).is_null() {
            return;
        }
        let req = s.alloc_request();
        unsafe {
            (*req).provider = provider;
            (*req).expected_crc = expected_crc;
            (*req).padding = padding;
        }
        if urgent {
            unsafe { s.pending_urgent_queue.put(linkable_of(req), key) };
            s.pending_urgent_queue_size += 1;
        } else {
            unsafe { s.request_queue.push(linkable2_of(req)) };
            unsafe { s.pending_prefetch_queue.put(linkable_of(req), key) };
            s.pending_prefetch_queue_size += 1;
        }
    } else if urgent {
        unsafe {
            let req = req_from_linkable(pending_prefetch);
            (*linkable2_of(req)).unlink2();
            s.pending_urgent_queue.put(linkable_of(req), key);
        }
        s.pending_prefetch_queue_size -= 1;
        s.pending_urgent_queue_size += 1;
    }
}

// @ObfuscatedName("ab.c(IIS)V")
pub fn update_cache_hint(archive_id: i32, group_id: i32) {
    let mut s = STATE.lock().unwrap();
    let key = ((archive_id as i64) << 16) + group_id as i64;
    let p = s.pending_prefetch_queue.find(key);
    if !p.is_null() {
        let req = unsafe { req_from_linkable(p) };
        unsafe { s.request_queue.push_front(linkable2_of(req)) };
    }
}

// @ObfuscatedName("v.n(III)I")
pub fn transfer_progress(archive_id: i32, group_id: i32) -> i32 {
    let s = STATE.lock().unwrap();
    let key = ((archive_id as i64) << 16) + group_id as i64;
    if !s.incoming_request.is_null()
        && unsafe { (*super_linkable(s.incoming_request)).key } == key
        && s.incoming_group_buffer.is_some()
    {
        let buf = s.incoming_group_buffer.as_ref().unwrap();
        let padding = unsafe { (*s.incoming_request).padding } as i32;
        buf.pos * 99 / (buf.data.len() as i32 - padding) + 1
    } else {
        0
    }
}

unsafe fn super_linkable(r: *mut Js5NetRequest) -> *mut crate::datastruct::linkable::Linkable {
    unsafe { &mut (*r).base.base as *mut _ }
}

// custom helper — `Js5Net.urgentQueueSize()` in Java.
pub fn urgent_queue_size_total() -> i32 {
    let s = STATE.lock().unwrap();
    s.urgent_queue_size + s.pending_urgent_queue_size
}

pub fn assign_loader_slot(archive: i32, slot: i32) {
    let mut s = STATE.lock().unwrap();
    s.field1200[archive as usize] = slot;
}

pub fn has_master_index() -> bool {
    STATE.lock().unwrap().master_index_buffer.is_some()
}

pub fn read_master_index_at(archive: i32) -> (i32, i32) {
    let mut s = STATE.lock().unwrap();
    let mib = s.master_index_buffer.as_mut().unwrap();
    mib.pos = (archive * 8 + 5) as i32;
    let crc = mib.g4();
    let ver = mib.g4();
    (crc, ver)
}

pub fn crc_error_count() -> i32 {
    STATE.lock().unwrap().crc_error_count
}

pub fn io_error_count() -> i32 {
    STATE.lock().unwrap().io_error_count
}

pub fn set_io_error_count(n: i32) {
    STATE.lock().unwrap().io_error_count = n;
}

pub fn has_stream() -> bool {
    STATE.lock().unwrap().stream.is_some()
}

pub fn clear_stream() {
    let mut s = STATE.lock().unwrap();
    if let Some(stream) = s.stream.as_mut() {
        stream.close();
    }
    s.stream = None;
}

// The Java code lets Client check `Js5Net.stream != null` directly; we
// expose that through `has_stream()` above. The actual ClientStream lives
// inside STATE so the loop owns the read-side cursor uncontested.
pub fn _ioerror_translate(_: IoError) {}
