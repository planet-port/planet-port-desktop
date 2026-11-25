//! 进程内事件环形缓冲(容量 1024,满则丢弃最旧)。
//! 查询语义(REST 契约):按 planet 过滤(可省)、ts > after(可省)、
//! 最新在前取 limit(默认 40 由 handler 决定)。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use planet_port_api::types::EventEntry;

const RING_CAPACITY: usize = 1024;

pub struct Ring {
    buf: Mutex<VecDeque<EventEntry>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

impl Ring {
    pub fn new() -> Self {
        Self {
            buf: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
        }
    }

    pub fn push(&self, planet: &str, kind: &str, message: &str) {
        let mut buf = self.buf.lock().unwrap();
        if buf.len() >= RING_CAPACITY {
            buf.pop_front();
        }
        buf.push_back(EventEntry {
            planet: planet.into(),
            ts_ms: now_ms(),
            kind: kind.into(),
            message: message.into(),
        });
    }

    pub fn tail(
        &self,
        planet_filter: Option<&str>,
        after: Option<u64>,
        limit: usize,
    ) -> Vec<EventEntry> {
        let buf = self.buf.lock().unwrap();
        buf.iter()
            .filter(|e| planet_filter.map(|f| e.planet == f).unwrap_or(true))
            .filter(|e| after.map(|a| e.ts_ms > a).unwrap_or(true))
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}
