use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

const START_MARKER: &str = "<codeclaw-actions>";
const END_MARKER: &str = "</codeclaw-actions>";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MasterEnvelope {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub actions: Vec<MasterAction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MasterAction {
    SpawnWorker {
        group: String,
        task: String,
        #[serde(default)]
        summary: Option<String>,
        #[serde(default)]
        prompt: Option<String>,
    },
    SendWorkerPrompt {
        worker_id: String,
        prompt: String,
    },
    UpdateWorkerSummary {
        worker_id: String,
        summary: String,
    },
}

#[derive(Debug, Clone)]
pub struct ParsedMasterResponse {
    pub visible_response: String,
    pub envelope: MasterEnvelope,
}

pub fn visible_stream_text(raw: &str) -> &str {
    if let Some(start) = raw.find(START_MARKER) {
        return &raw[..start];
    }

    let max_prefix = raw.len().min(START_MARKER.len().saturating_sub(1));
    for prefix_len in (1..=max_prefix).rev() {
        if raw.ends_with(&START_MARKER[..prefix_len]) {
            let visible_len = raw.len() - prefix_len;
            return &raw[..visible_len];
        }
    }

    raw
}

pub fn parse_master_response(raw: &str) -> Result<ParsedMasterResponse> {
    let Some(start) = raw.rfind(START_MARKER) else {
        return Ok(ParsedMasterResponse {
            visible_response: raw.trim().to_owned(),
            envelope: MasterEnvelope::default(),
        });
    };
    let Some(relative_end) = raw[start..].find(END_MARKER) else {
        return Err(anyhow!(
            "master response contains an opening codeclaw action block without a closing marker"
        ));
    };
    let end = start + relative_end;
    let payload = raw[start + START_MARKER.len()..end].trim();
    let visible = raw[..start].trim().to_owned();

    let envelope: MasterEnvelope = serde_json::from_str(payload)
        .with_context(|| "failed to parse codeclaw action block as JSON")?;

    Ok(ParsedMasterResponse {
        visible_response: visible,
        envelope,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_master_response, visible_stream_text, MasterAction};

    #[test]
    fn parses_response_with_actions_block() {
        let parsed = parse_master_response(
            "I will split this into two workers.\n<codeclaw-actions>\n{\"summary\":\"Split the task\",\"actions\":[{\"type\":\"spawn_worker\",\"group\":\"backend\",\"task\":\"Refactor API\",\"summary\":\"Own API refactor\",\"prompt\":\"Start with handlers\"}]}\n</codeclaw-actions>",
        )
        .expect("parse should succeed");

        assert_eq!(
            parsed.visible_response,
            "I will split this into two workers."
        );
        assert_eq!(parsed.envelope.summary.as_deref(), Some("Split the task"));
        assert_eq!(parsed.envelope.actions.len(), 1);

        match &parsed.envelope.actions[0] {
            MasterAction::SpawnWorker {
                group,
                task,
                summary,
                prompt,
            } => {
                assert_eq!(group, "backend");
                assert_eq!(task, "Refactor API");
                assert_eq!(summary.as_deref(), Some("Own API refactor"));
                assert_eq!(prompt.as_deref(), Some("Start with handlers"));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn accepts_plain_response_without_actions() {
        let parsed = parse_master_response("No orchestration needed right now.")
            .expect("plain responses should parse");
        assert_eq!(
            parsed.visible_response,
            "No orchestration needed right now."
        );
        assert!(parsed.envelope.actions.is_empty());
    }

    #[test]
    fn hides_partial_action_marker_during_streaming() {
        assert_eq!(
            visible_stream_text("Planning the split.\n<codecl"),
            "Planning the split.\n"
        );
        assert_eq!(
            visible_stream_text(
                "Planning the split.\n<codeclaw-actions>\n{\"summary\":\"x\",\"actions\":[]}"
            ),
            "Planning the split.\n"
        );
    }
}
