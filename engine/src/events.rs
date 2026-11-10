use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Sink for Desktop events. The subprocess runner, the budget / convergence /
/// scope observers and the Tauri bridge all speak this trait; nothing in the
/// engine depends on the CLI crates' event types anymore.
pub trait DesktopEventSink: Send + Sync {
    fn emit_desktop(&self, ev: DesktopEvent);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,

    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesktopEvent {
    AssistantMessage {
        text: Option<String>,
        tool_calls: Vec<ToolCallInfo>,
        parent_tool_use_id: Option<String>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        parent_tool_use_id: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ApiRetry {
        attempt: u32,
        max_retries: u32,
        retry_delay_ms: u64,
        error_status: Option<u16>,
        error: String,
    },
    CompactBoundary {
        trigger: String,
        pre_tokens: u64,
    },
    PermissionDenied {
        tool_name: String,
        tool_use_id: String,
    },
    SystemMessage {
        event: String,
        text: String,
    },

    Unknown {
        variant: String,
    },

    SessionEnd {
        success: bool,
        result: String,
        num_turns: u64,
        total_cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        model: String,
        duration_ms: u64,

        claims_verified: u64,
        claims_unverified: u64,
        claims_contradicted: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub seq: u64,
    #[serde(flatten)]
    pub event: DesktopEvent,
}

#[derive(Default)]
pub struct RecordingSink {
    inner: Mutex<Vec<Envelope>>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn events(&self) -> Vec<Envelope> {
        self.inner.lock().unwrap().clone()
    }

    pub fn to_ndjson(&self) -> anyhow::Result<String> {
        let mut out = String::new();
        for e in self.events().iter() {
            let line = serde_json::to_string(e)?;
            out.push_str(&line);
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
        Ok(out)
    }
}

impl DesktopEventSink for RecordingSink {
    fn emit_desktop(&self, ev: DesktopEvent) {
        let mut g = self.inner.lock().unwrap();
        let seq = g.len() as u64;
        g.push(Envelope {
            v: SCHEMA_VERSION,
            seq,
            event: ev,
        });
    }
}

pub fn parse_ndjson(s: &str) -> anyhow::Result<Vec<Envelope>> {
    let mut out = vec![];
    for (i, line) in s.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let raw: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("line {}: invalid JSON: {e}", i + 1))?;
        let v = raw.get("v").and_then(|v| v.as_u64()).unwrap_or(0);
        if v == 0 || v > SCHEMA_VERSION as u64 {
            anyhow::bail!("line {}: unsupported schema version {v}", i + 1);
        }
        out.push(
            serde_json::from_value(raw)
                .map_err(|e| anyhow::anyhow!("line {}: bad event: {e}", i + 1))?,
        );
    }
    Ok(out)
}
