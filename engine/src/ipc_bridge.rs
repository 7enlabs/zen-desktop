use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::events::{parse_ndjson, DesktopEvent, DesktopEventSink, Envelope, SCHEMA_VERSION};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcEnvelope {
    pub v: u32,
    pub seq: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(flatten)]
    pub payload: serde_json::Value,
}

impl IpcEnvelope {
    pub fn new(seq: u64, event: &DesktopEvent) -> anyhow::Result<Self> {
        let mut value = serde_json::to_value(event)
            .map_err(|e| anyhow::anyhow!("event not serializable: {e}"))?;
        let obj = value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("event serialized to non-object"))?;
        let event_type = obj
            .remove("type")
            .and_then(|t| t.as_str().map(str::to_owned))
            .ok_or_else(|| anyhow::anyhow!("event missing type tag"))?;
        Ok(IpcEnvelope {
            v: SCHEMA_VERSION,
            seq,
            event_type,
            payload: serde_json::Value::Object(obj.clone()),
        })
    }

    pub fn event_type_of(event: &DesktopEvent) -> String {
        match event {
            DesktopEvent::AssistantMessage { .. } => "assistant_message",
            DesktopEvent::ToolResult { .. } => "tool_result",
            DesktopEvent::TextDelta { .. } => "text_delta",
            DesktopEvent::ApiRetry { .. } => "api_retry",
            DesktopEvent::CompactBoundary { .. } => "compact_boundary",
            DesktopEvent::PermissionDenied { .. } => "permission_denied",
            DesktopEvent::SystemMessage { .. } => "system_message",
            DesktopEvent::SessionEnd { .. } => "session_end",
            DesktopEvent::Unknown { .. } => "unknown",
        }
        .to_owned()
    }

    pub fn to_event(&self) -> anyhow::Result<DesktopEvent> {
        let mut value = self.payload.clone();
        let obj = value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("payload is not an object"))?;
        obj.insert(
            "type".to_owned(),
            serde_json::Value::String(self.event_type.clone()),
        );
        serde_json::from_value(value).map_err(|e| anyhow::anyhow!("bad event payload: {e}"))
    }

    pub fn to_envelope(&self) -> anyhow::Result<Envelope> {
        Ok(Envelope {
            v: self.v,
            seq: self.seq,
            event: self.to_event()?,
        })
    }

    pub fn from_envelope(env: &Envelope) -> anyhow::Result<Self> {
        IpcEnvelope::new(env.seq, &env.event)
    }
}

pub struct BridgeSink {
    seq: AtomicU64,
    log: Mutex<Vec<IpcEnvelope>>,
}

impl BridgeSink {
    pub fn new() -> Self {
        BridgeSink {
            seq: AtomicU64::new(0),
            log: Mutex::new(Vec::new()),
        }
    }

    pub fn envelopes(&self) -> Vec<IpcEnvelope> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn get_log_since(&self, since: u64) -> Vec<IpcEnvelope> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|e| e.seq > since)
            .cloned()
            .collect()
    }

    pub fn to_ndjson(&self) -> anyhow::Result<String> {
        self.envelopes()
            .iter()
            .map(|e| {
                serde_json::to_string(e).map_err(|err| {
                    anyhow::anyhow!("envelope seq {} not serializable: {err}", e.seq)
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .map(|lines| lines.join("\n"))
    }

    pub fn from_ndjson(s: &str) -> anyhow::Result<Self> {
        let envelopes: Vec<Envelope> = parse_ndjson(s)?;
        let mut log: Vec<IpcEnvelope> = envelopes
            .iter()
            .map(IpcEnvelope::from_envelope)
            .collect::<Result<_, _>>()?;
        log.sort_by_key(|e| e.seq);
        let next = log.last().map(|e| e.seq + 1).unwrap_or(0);
        Ok(BridgeSink {
            seq: AtomicU64::new(next),
            log: Mutex::new(log),
        })
    }
}

impl Default for BridgeSink {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopEventSink for BridgeSink {
    fn emit_desktop(&self, ev: DesktopEvent) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        if let Ok(env) = IpcEnvelope::new(seq, &ev) {
            self.log.lock().unwrap_or_else(|e| e.into_inner()).push(env);
        }
    }
}

pub fn detect_gaps(received: &[u64]) -> Vec<u64> {
    let Some(&max) = received.iter().max() else {
        return vec![];
    };
    let seen: std::collections::HashSet<u64> = received.iter().copied().collect();
    (0..=max).filter(|s| !seen.contains(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(text: &str) -> DesktopEvent {
        DesktopEvent::TextDelta {
            text: text.to_owned(),
        }
    }

    #[test]
    fn envelope_splits_type_from_payload() {
        let env = IpcEnvelope::new(42, &delta("Hello, ")).unwrap();
        assert_eq!(env.v, SCHEMA_VERSION);
        assert_eq!(env.seq, 42);
        assert_eq!(env.event_type, "text_delta");
        assert_eq!(env.payload["text"], "Hello, ");

        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains(r#""type":"text_delta""#), "{json}");
        assert!(!json.contains("event_type"), "{json}");
    }

    #[test]
    fn envelope_roundtrip() {
        let event = DesktopEvent::ToolResult {
            tool_use_id: "tu_1".into(),
            content: "ok".into(),
            is_error: false,
            parent_tool_use_id: None,
        };
        let env = IpcEnvelope::new(7, &event).unwrap();
        assert_eq!(env.to_event().unwrap(), event);
        assert_eq!(env.to_envelope().unwrap().seq, 7);
    }

    #[test]
    fn event_type_of_covers_all_variants() {
        let variants = vec![
            DesktopEvent::AssistantMessage {
                text: None,
                tool_calls: vec![],
                parent_tool_use_id: None,
            },
            delta("x"),
            DesktopEvent::ApiRetry {
                attempt: 1,
                max_retries: 3,
                retry_delay_ms: 100,
                error_status: None,
                error: "e".into(),
            },
            DesktopEvent::CompactBoundary {
                trigger: "t".into(),
                pre_tokens: 1,
            },
            DesktopEvent::PermissionDenied {
                tool_name: "bash".into(),
                tool_use_id: "u".into(),
            },
            DesktopEvent::SystemMessage {
                event: "e".into(),
                text: "t".into(),
            },
        ];
        for v in &variants {
            let tag = IpcEnvelope::event_type_of(v);
            assert!(!tag.is_empty());

            assert_eq!(IpcEnvelope::new(0, v).unwrap().event_type, tag);
        }
    }

    #[test]
    fn bridge_sink_seq_is_monotonic() {
        let sink = BridgeSink::new();
        for i in 0..5 {
            sink.emit_desktop(DesktopEvent::TextDelta {
                text: format!("t{i}"),
            });
        }
        let envs = sink.envelopes();
        let seqs: Vec<u64> = envs.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn bridge_sink_ndjson_roundtrip() {
        let sink = BridgeSink::new();
        sink.emit_desktop(DesktopEvent::TextDelta { text: "a".into() });
        sink.emit_desktop(DesktopEvent::TextDelta { text: "b".into() });
        let ndjson = sink.to_ndjson().unwrap();
        assert_eq!(ndjson.lines().count(), 2);

        let restored = BridgeSink::from_ndjson(&ndjson).unwrap();
        assert_eq!(restored.envelopes().len(), 2);

        restored.emit_desktop(DesktopEvent::TextDelta { text: "c".into() });
        let seqs: Vec<u64> = restored.envelopes().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }

    #[test]
    fn get_log_since_returns_tail() {
        let sink = BridgeSink::new();
        for i in 0..5 {
            sink.emit_desktop(DesktopEvent::TextDelta {
                text: format!("t{i}"),
            });
        }
        let tail = sink.get_log_since(2);
        let seqs: Vec<u64> = tail.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![3, 4]);
        assert!(sink.get_log_since(99).is_empty());
    }

    #[test]
    fn detect_gaps_finds_missing() {
        assert_eq!(detect_gaps(&[]), Vec::<u64>::new());
        assert_eq!(detect_gaps(&[0, 1, 2]), Vec::<u64>::new());
        assert_eq!(detect_gaps(&[0, 2, 5]), vec![1, 3, 4]);

        assert_eq!(detect_gaps(&[3, 1, 3, 0]), vec![2]);
    }

    #[test]
    fn bridge_sink_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BridgeSink>();
    }
}
