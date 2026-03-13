# CodeClaw Gateway Protocol

## Purpose

This document defines the channel-neutral gateway contract used by CodeClaw for IM, webhook, and future remote-control adapters.

The goal is compatibility first:

- keep one canonical internal schema
- preserve platform-specific `type`, `event`, and `hook` semantics
- support text and rich media without forcing every channel into the same rendering model
- allow graceful downgrade when a target platform lacks a capability

## Design Principles

### 1. Normalize, then adapt

Inbound traffic should be normalized into one shared message/event shape before job logic runs.

Outbound reports should be rendered from one shared envelope shape before any platform-specific adapter sends them.

### 2. Preserve raw semantics

Adapters must not throw away upstream platform meaning when it exists.

Each inbound event should preserve:

- `raw_type`
- `raw_event`
- `raw_hook`

This allows Slack-style events, Telegram updates, webhook callbacks, IM bot hooks, and custom gateway events to retain their original meaning for audit or debugging.

### 3. Content blocks over flat strings

Many IM systems support more than plain text.

The normalized model therefore supports ordered content blocks instead of assuming a single text field.

### 4. Fallback text is mandatory

Every outbound envelope must provide `fallback_text`.

This guarantees that platforms with weak rendering, plain-text notifications, or operator log views can still show a usable message.

### 5. Capability negotiation is explicit

Adapters must declare capabilities so CodeClaw can decide whether to:

- send markdown or plain text
- attach media or replace it with links
- emit typing indicators
- preserve raw type/event/hook metadata

## Canonical Types

### Gateway Envelope Kinds

Supported normalized envelope kinds:

- `message`
- `command`
- `event`
- `hook`
- `typing`
- `delivery_receipt`
- `unknown`

### Content Block Kinds

Supported normalized content block kinds:

- `text`
- `markdown`
- `link`
- `image`
- `audio`
- `video`
- `file`

### Typing State

Typing is modeled separately from content payloads:

- `started`
- `stopped`

This lets adapters map to platform-native typing APIs when available, or ignore the field when unsupported.

## Inbound Event Contract

Inbound traffic is normalized into:

```json
{
  "adapter_id": "example-im",
  "platform": "generic",
  "envelope_kind": "hook",
  "raw_type": "message.created",
  "raw_event": "message",
  "raw_hook": "created",
  "message": {
    "message_id": "msg-001",
    "conversation": {
      "id": "conv-001",
      "title": "Engineering Chat",
      "thread_id": "thread-17"
    },
    "sender": {
      "id": "user-001",
      "display_name": "Operator",
      "is_bot": false
    },
    "blocks": [
      {
        "kind": "markdown",
        "text": "Please continue the **payment API** work."
      },
      {
        "kind": "link",
        "url": "https://github.com/nikkofu/codeclaw/issues/42",
        "title": "linked issue"
      },
      {
        "kind": "image",
        "url": "https://example.invalid/diagram.png",
        "alt": "system architecture diagram",
        "mime_type": "image/png"
      }
    ],
    "fallback_text": "Please continue the payment API work.",
    "markdown": "Please continue the **payment API** work.",
    "links": [
      "https://github.com/nikkofu/codeclaw/issues/42"
    ],
    "created_at": 1773300000
  },
  "metadata": {
    "type": "message.created",
    "event": "message",
    "hook": "created"
  }
}
```

## Outbound Envelope Contract

CodeClaw reports and proactive updates are normalized into:

```json
{
  "conversation": {
    "id": "JOB-001",
    "title": "job:JOB-001",
    "thread_id": null
  },
  "typing": {
    "state": "started",
    "ttl_ms": 3000
  },
  "blocks": [
    {
      "kind": "markdown",
      "text": "Job **JOB-001** is running."
    },
    {
      "kind": "link",
      "url": "https://github.com/nikkofu/codeclaw",
      "title": "repository"
    },
    {
      "kind": "audio",
      "url": "https://example.invalid/voice-summary.mp3",
      "transcript": "Voice summary transcript",
      "mime_type": "audio/mpeg",
      "duration_ms": 4200
    }
  ],
  "fallback_text": "Job JOB-001 is running.",
  "markdown": "Job **JOB-001** is running.",
  "links": [
    "https://github.com/nikkofu/codeclaw"
  ],
  "metadata": {
    "job_id": "JOB-001",
    "report_kind": "progress"
  }
}
```

## Compatibility Rules

### Text and Markdown

- `fallback_text` is required for every outbound envelope
- `markdown` is optional and should be used only when the target declares markdown support
- adapters should downgrade markdown to plain text when necessary

### Links

- links can appear as dedicated `link` blocks
- `links` provides a flat list for platforms that prefer plain URL arrays
- adapters may inline links into text when block rendering is not supported

### Image, Audio, Video, and File

- media is represented as explicit blocks with URLs and optional metadata
- adapters may replace unsupported media with a plain link plus explanatory text
- file attachments should preserve filename, MIME type, and size when known

### Typing Indicators

- typing is modeled independently from message sending
- adapters may ignore `typing` if the platform lacks native support
- long-running jobs should prefer short-lived typing pulses instead of one indefinite indicator

### `type`, `event`, and `hook`

CodeClaw treats these fields as first-class compatibility primitives.

Recommended mapping:

- `raw_type`: original upstream or downstream event type
- `raw_event`: coarse event family
- `raw_hook`: sub-event or callback stage

Examples:

- Slack-style event callbacks: `raw_type=event_callback`, `raw_event=message`, `raw_hook=created`
- Telegram-style updates: `raw_type=message`, `raw_event=chat`, `raw_hook=text`
- custom IM gateway webhooks: `raw_type=message.created`, `raw_event=message`, `raw_hook=created`

## Capability Declaration

Each adapter should publish a `GatewayCapabilities` record.

Current CodeClaw channels:

### `console`

- text: yes
- markdown: yes
- links: yes
- image/audio/video/file: no
- typing: yes
- raw type/event/hook: yes

### `mock_file`

- text: yes
- markdown: yes
- links: yes
- image/audio/video/file: yes
- typing: yes
- raw type/event/hook: yes

The `mock_file` channel exists as a delivery-safe reference adapter for schema validation, integration testing, and future IM gateway development.

## Adapter Mapping Guidance

When implementing a real IM adapter:

1. Normalize every inbound event into the shared schema before job routing.
2. Preserve raw platform metadata under `raw_type`, `raw_event`, `raw_hook`, and `metadata`.
3. Advertise true capabilities only.
4. Downgrade unsupported blocks instead of dropping the whole message.
5. Keep message rendering deterministic so audits match what operators received.

## Current CLI Support

CodeClaw exposes the protocol through:

```bash
cargo run -- gateway schema
cargo run -- gateway capabilities --channel console
cargo run -- gateway capabilities --channel mock-file
cargo run -- gateway subscribe --job JOB-001 --channel mock-file
```

## Future Platform Targets

The protocol is designed so the next adapters can be added without changing the job model:

- Slack
- Telegram
- WeCom
- Feishu
- Discord
- webhook relays
- custom IM gateway bridges
