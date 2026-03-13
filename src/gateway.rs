use crate::state::{JobReportRecord, ReportChannel};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GatewayPlatform {
    Console,
    MockFile,
    Slack,
    Telegram,
    Wecom,
    Feishu,
    Discord,
    Webhook,
    Generic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GatewayEnvelopeKind {
    Message,
    Command,
    Event,
    Hook,
    Typing,
    DeliveryReceipt,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GatewayContentKind {
    Text,
    Markdown,
    Link,
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GatewayTypingState {
    Started,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayCapabilities {
    pub platform: GatewayPlatform,
    pub supports_text: bool,
    pub supports_markdown: bool,
    pub supports_links: bool,
    pub supports_images: bool,
    pub supports_audio: bool,
    pub supports_video: bool,
    pub supports_files: bool,
    pub supports_typing: bool,
    pub supports_raw_type: bool,
    pub supports_raw_event: bool,
    pub supports_raw_hook: bool,
    pub inbound_event_kinds: Vec<GatewayEnvelopeKind>,
    pub outbound_content_kinds: Vec<GatewayContentKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayActor {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub is_bot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConversationRef {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayTypingIndicator {
    pub state: GatewayTypingState,
    #[serde(default)]
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GatewayContentBlock {
    Text {
        text: String,
    },
    Markdown {
        text: String,
    },
    Link {
        url: String,
        #[serde(default)]
        title: Option<String>,
    },
    Image {
        url: String,
        #[serde(default)]
        alt: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
    },
    Audio {
        url: String,
        #[serde(default)]
        transcript: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        duration_ms: Option<u64>,
    },
    Video {
        url: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        poster_url: Option<String>,
    },
    File {
        url: String,
        name: String,
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        size_bytes: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedGatewayMessage {
    #[serde(default)]
    pub message_id: Option<String>,
    pub conversation: GatewayConversationRef,
    #[serde(default)]
    pub sender: Option<GatewayActor>,
    pub blocks: Vec<GatewayContentBlock>,
    pub fallback_text: String,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub links: Vec<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundGatewayEvent {
    pub adapter_id: String,
    pub platform: GatewayPlatform,
    pub envelope_kind: GatewayEnvelopeKind,
    pub raw_type: String,
    #[serde(default)]
    pub raw_event: Option<String>,
    #[serde(default)]
    pub raw_hook: Option<String>,
    #[serde(default)]
    pub message: Option<NormalizedGatewayMessage>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundGatewayEnvelope {
    pub conversation: GatewayConversationRef,
    #[serde(default)]
    pub typing: Option<GatewayTypingIndicator>,
    pub blocks: Vec<GatewayContentBlock>,
    pub fallback_text: String,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[allow(dead_code)]
pub trait GatewayAdapter {
    fn adapter_id(&self) -> &str;
    fn platform(&self) -> GatewayPlatform;
    fn capabilities(&self) -> GatewayCapabilities;
    fn deliver(&self, envelope: &OutboundGatewayEnvelope) -> Result<String>;
}

pub struct ConsoleGatewayAdapter {
    target: String,
}

pub struct MockFileGatewayAdapter {
    target: PathBuf,
}

impl ConsoleGatewayAdapter {
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl MockFileGatewayAdapter {
    pub fn new(target: impl Into<PathBuf>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl GatewayAdapter for ConsoleGatewayAdapter {
    fn adapter_id(&self) -> &str {
        "console"
    }

    fn platform(&self) -> GatewayPlatform {
        GatewayPlatform::Console
    }

    fn capabilities(&self) -> GatewayCapabilities {
        capabilities_for_channel(&ReportChannel::Console)
    }

    fn deliver(&self, envelope: &OutboundGatewayEnvelope) -> Result<String> {
        let rendered = envelope
            .markdown
            .clone()
            .unwrap_or_else(|| envelope.fallback_text.clone());
        println!("[gateway:{}] {}", self.target, rendered);
        Ok(format!("console:{}", self.target))
    }
}

impl GatewayAdapter for MockFileGatewayAdapter {
    fn adapter_id(&self) -> &str {
        "mock_file"
    }

    fn platform(&self) -> GatewayPlatform {
        GatewayPlatform::MockFile
    }

    fn capabilities(&self) -> GatewayCapabilities {
        capabilities_for_channel(&ReportChannel::MockFile)
    }

    fn deliver(&self, envelope: &OutboundGatewayEnvelope) -> Result<String> {
        if let Some(parent) = self.target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.target)
            .with_context(|| format!("failed to open {}", self.target.display()))?;
        let raw = serde_json::to_string(envelope).context("failed to encode gateway envelope")?;
        writeln!(file, "{raw}")
            .with_context(|| format!("failed to append {}", self.target.display()))?;
        Ok(format!("mock-file:{}", self.target.display()))
    }
}

impl fmt::Display for GatewayPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Console => "console",
            Self::MockFile => "mock_file",
            Self::Slack => "slack",
            Self::Telegram => "telegram",
            Self::Wecom => "wecom",
            Self::Feishu => "feishu",
            Self::Discord => "discord",
            Self::Webhook => "webhook",
            Self::Generic => "generic",
        };
        f.write_str(value)
    }
}

impl fmt::Display for GatewayEnvelopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Message => "message",
            Self::Command => "command",
            Self::Event => "event",
            Self::Hook => "hook",
            Self::Typing => "typing",
            Self::DeliveryReceipt => "delivery_receipt",
            Self::Unknown => "unknown",
        };
        f.write_str(value)
    }
}

impl fmt::Display for GatewayContentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Link => "link",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::File => "file",
        };
        f.write_str(value)
    }
}

pub fn capabilities_for_channel(channel: &ReportChannel) -> GatewayCapabilities {
    match channel {
        ReportChannel::Console => GatewayCapabilities {
            platform: GatewayPlatform::Console,
            supports_text: true,
            supports_markdown: true,
            supports_links: true,
            supports_images: false,
            supports_audio: false,
            supports_video: false,
            supports_files: false,
            supports_typing: true,
            supports_raw_type: true,
            supports_raw_event: true,
            supports_raw_hook: true,
            inbound_event_kinds: vec![
                GatewayEnvelopeKind::Message,
                GatewayEnvelopeKind::Command,
                GatewayEnvelopeKind::Event,
                GatewayEnvelopeKind::Hook,
                GatewayEnvelopeKind::Typing,
            ],
            outbound_content_kinds: vec![
                GatewayContentKind::Text,
                GatewayContentKind::Markdown,
                GatewayContentKind::Link,
            ],
        },
        ReportChannel::MockFile => GatewayCapabilities {
            platform: GatewayPlatform::MockFile,
            supports_text: true,
            supports_markdown: true,
            supports_links: true,
            supports_images: true,
            supports_audio: true,
            supports_video: true,
            supports_files: true,
            supports_typing: true,
            supports_raw_type: true,
            supports_raw_event: true,
            supports_raw_hook: true,
            inbound_event_kinds: vec![
                GatewayEnvelopeKind::Message,
                GatewayEnvelopeKind::Command,
                GatewayEnvelopeKind::Event,
                GatewayEnvelopeKind::Hook,
                GatewayEnvelopeKind::Typing,
                GatewayEnvelopeKind::DeliveryReceipt,
            ],
            outbound_content_kinds: vec![
                GatewayContentKind::Text,
                GatewayContentKind::Markdown,
                GatewayContentKind::Link,
                GatewayContentKind::Image,
                GatewayContentKind::Audio,
                GatewayContentKind::Video,
                GatewayContentKind::File,
            ],
        },
    }
}

pub fn sample_inbound_event() -> InboundGatewayEvent {
    InboundGatewayEvent {
        adapter_id: "example-im".to_owned(),
        platform: GatewayPlatform::Generic,
        envelope_kind: GatewayEnvelopeKind::Hook,
        raw_type: "message.created".to_owned(),
        raw_event: Some("message".to_owned()),
        raw_hook: Some("created".to_owned()),
        message: Some(NormalizedGatewayMessage {
            message_id: Some("msg-001".to_owned()),
            conversation: GatewayConversationRef {
                id: "conv-001".to_owned(),
                title: Some("Engineering Chat".to_owned()),
                thread_id: Some("thread-17".to_owned()),
            },
            sender: Some(GatewayActor {
                id: "user-001".to_owned(),
                display_name: Some("Operator".to_owned()),
                is_bot: false,
            }),
            blocks: vec![
                GatewayContentBlock::Markdown {
                    text: "Please continue the **payment API** work.".to_owned(),
                },
                GatewayContentBlock::Link {
                    url: "https://github.com/nikkofu/codeclaw/issues/42".to_owned(),
                    title: Some("linked issue".to_owned()),
                },
                GatewayContentBlock::Image {
                    url: "https://example.invalid/diagram.png".to_owned(),
                    alt: Some("system architecture diagram".to_owned()),
                    mime_type: Some("image/png".to_owned()),
                },
            ],
            fallback_text: "Please continue the payment API work.".to_owned(),
            markdown: Some("Please continue the **payment API** work.".to_owned()),
            links: vec!["https://github.com/nikkofu/codeclaw/issues/42".to_owned()],
            created_at: 1773300000,
        }),
        metadata: BTreeMap::from([
            ("type".to_owned(), "message.created".to_owned()),
            ("event".to_owned(), "message".to_owned()),
            ("hook".to_owned(), "created".to_owned()),
        ]),
    }
}

pub fn sample_outbound_envelope() -> OutboundGatewayEnvelope {
    OutboundGatewayEnvelope {
        conversation: GatewayConversationRef {
            id: "conv-001".to_owned(),
            title: Some("Engineering Chat".to_owned()),
            thread_id: Some("thread-17".to_owned()),
        },
        typing: Some(GatewayTypingIndicator {
            state: GatewayTypingState::Started,
            ttl_ms: Some(3000),
        }),
        blocks: vec![
            GatewayContentBlock::Markdown {
                text: "Job **JOB-001** is running.\n\n- status: running\n- next step: continue implementation".to_owned(),
            },
            GatewayContentBlock::Link {
                url: "https://github.com/nikkofu/codeclaw".to_owned(),
                title: Some("repository".to_owned()),
            },
            GatewayContentBlock::Audio {
                url: "https://example.invalid/voice-summary.mp3".to_owned(),
                transcript: Some("Voice summary transcript".to_owned()),
                mime_type: Some("audio/mpeg".to_owned()),
                duration_ms: Some(4200),
            },
            GatewayContentBlock::Video {
                url: "https://example.invalid/demo.mp4".to_owned(),
                title: Some("demo clip".to_owned()),
                mime_type: Some("video/mp4".to_owned()),
                duration_ms: Some(12000),
                poster_url: Some("https://example.invalid/demo.jpg".to_owned()),
            },
        ],
        fallback_text: "Job JOB-001 is running.".to_owned(),
        markdown: Some(
            "Job **JOB-001** is running.\n\n- status: running\n- next step: continue implementation"
                .to_owned(),
        ),
        links: vec!["https://github.com/nikkofu/codeclaw".to_owned()],
        metadata: BTreeMap::from([
            ("job_id".to_owned(), "JOB-001".to_owned()),
            ("report_kind".to_owned(), "progress".to_owned()),
        ]),
    }
}

pub fn default_target_for_channel(channel: &ReportChannel, root: &Path) -> String {
    match channel {
        ReportChannel::Console => "stdout".to_owned(),
        ReportChannel::MockFile => default_mock_outbox_path(root).display().to_string(),
    }
}

pub fn default_mock_outbox_path(root: &Path) -> PathBuf {
    root.join("gateway").join("mock-outbox.jsonl")
}

pub fn report_envelope(report: &JobReportRecord) -> OutboundGatewayEnvelope {
    let markdown = format!(
        "Job **{}** report\n\n- kind: `{}`\n- status: `{}`\n- summary: {}\n\n{}",
        report.job_id, report.kind, report.job_status, report.summary, report.body
    );
    OutboundGatewayEnvelope {
        conversation: GatewayConversationRef {
            id: report.job_id.clone(),
            title: Some(format!("job:{}", report.job_id)),
            thread_id: None,
        },
        typing: None,
        blocks: vec![GatewayContentBlock::Markdown {
            text: markdown.clone(),
        }],
        fallback_text: report.summary.clone(),
        markdown: Some(markdown),
        links: Vec::new(),
        metadata: BTreeMap::from([
            ("job_id".to_owned(), report.job_id.clone()),
            ("report_id".to_owned(), format!("RPT-{:03}", report.id)),
            ("report_kind".to_owned(), report.kind.to_string()),
            ("job_status".to_owned(), report.job_status.to_string()),
        ]),
    }
}

pub fn deliver_report(
    channel: &ReportChannel,
    target: &str,
    root: &Path,
    report: &JobReportRecord,
) -> Result<String> {
    let envelope = report_envelope(report);
    match channel {
        ReportChannel::Console => ConsoleGatewayAdapter::new(target).deliver(&envelope),
        ReportChannel::MockFile => {
            let path = if target.trim().is_empty() || target == "default" {
                default_mock_outbox_path(root)
            } else {
                PathBuf::from(target)
            };
            MockFileGatewayAdapter::new(path).deliver(&envelope)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        capabilities_for_channel, report_envelope, sample_inbound_event, sample_outbound_envelope,
    };
    use crate::state::{JobReportKind, JobReportRecord, JobStatus, ReportChannel};

    #[test]
    fn mock_channel_capabilities_include_multimedia_and_typing() {
        let capabilities = capabilities_for_channel(&ReportChannel::MockFile);
        assert!(capabilities.supports_images);
        assert!(capabilities.supports_audio);
        assert!(capabilities.supports_video);
        assert!(capabilities.supports_typing);
        assert!(capabilities.supports_raw_hook);
    }

    #[test]
    fn sample_gateway_schema_contains_type_event_hook_and_media() {
        let inbound = sample_inbound_event();
        assert_eq!(inbound.raw_type, "message.created");
        assert_eq!(inbound.raw_event.as_deref(), Some("message"));
        assert_eq!(inbound.raw_hook.as_deref(), Some("created"));

        let outbound = sample_outbound_envelope();
        assert!(outbound.typing.is_some());
        assert!(outbound.markdown.is_some());
        assert!(outbound.blocks.len() >= 4);
    }

    #[test]
    fn report_envelope_maps_report_to_markdown_message() {
        let envelope = report_envelope(&JobReportRecord {
            id: 1,
            job_id: "JOB-001".to_owned(),
            kind: JobReportKind::Progress,
            job_status: JobStatus::Running,
            summary: "progress: implemented handlers".to_owned(),
            body: "Detailed job progress body".to_owned(),
            created_at: 1,
        });

        assert_eq!(envelope.conversation.id, "JOB-001");
        assert_eq!(envelope.fallback_text, "progress: implemented handlers");
        assert!(envelope
            .markdown
            .as_deref()
            .is_some_and(|text| text.contains("Detailed job progress body")));
    }
}
