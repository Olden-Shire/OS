// @ObfuscatedName("fa") — jag::JagException.
//
// Verbatim port of the simpler half of JagException.java — the
// `report(Throwable, String)` factory (line 104-113) that wraps or
// re-wraps an existing error with a fresh message. The full
// stack-trace upload path (the 60-line static report) hits AWT
// applet APIs that have no Rust equivalent — when porting the crash
// report endpoint we'll route to a custom signlink::report().

#![allow(dead_code)]

use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct GlobalReportState {
    // @ObfuscatedName("fa.d") — username stamped into clienterror.ws URL.
    pub username: String,
    // @ObfuscatedName("fa.l") — rev number, same source as protocol.
    pub revision: i32,
}

pub static REPORT_STATE: Mutex<GlobalReportState> = Mutex::new(GlobalReportState {
    username: String::new(),
    revision: 0,
});

#[derive(Debug, Clone)]
pub struct JagException {
    // @ObfuscatedName("fa.m")
    pub message: String,
    // @ObfuscatedName("fa.c") — Java's `cause` is the original
    // Throwable. We don't have boxed errors here so we keep the
    // cause as a stringified form for crash reports.
    pub cause: String,
}

impl JagException {
    pub fn new(cause: String, message: String) -> Self {
        Self { message, cause }
    }

    // @ObfuscatedName("bh.d(Ljava/lang/Throwable;Ljava/lang/String;)Lfa;") —
    // JagException.report(Throwable, String). Verbatim port of
    // JagException.java:104-113. If `cause` is already a JagException,
    // append the message; otherwise wrap.
    pub fn report(cause: Option<JagException>, message: &str) -> JagException {
        match cause {
            Some(mut existing) => {
                existing.message.push(' ');
                existing.message.push_str(message);
                existing
            }
            None => JagException::new(String::new(), message.to_string()),
        }
    }
}
