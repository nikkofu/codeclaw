# CodeClaw v0.13.1 Community Post Kit

## Purpose

This document packages ready-to-use outward-facing copy for sharing CodeClaw `v0.13.1`.

Use it for:

- GitHub Discussions or project updates
- X / Twitter
- LinkedIn
- IM group announcements
- release recap emails

Canonical release page:

- `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`

Repository:

- `https://github.com/nikkofu/codeclaw`

## Core Message

CodeClaw is a terminal-first control plane for supervising one master Codex session and multiple worker sessions. The `0.13.x` line introduced stronger local monitoring, bounded session automations, a more capable onboard control surface, and better operator UX. `v0.13.1` freezes that full package into a clean tagged release with complete delivery materials.

## Key Points

- local monitor answers come from CodeClaw state instead of a model guess
- onboard exposes `Codex Sessions` and `Automations` panels
- session automation is bounded and explicit
- `up` can drive scheduler ticks while the TUI stays open
- `inspect --service` exposes runtime heartbeat details
- the tagged release now includes release announcement and upgrade notes

## Long-Form Release Post

```md
CodeClaw v0.13.1 is now available.

CodeClaw is a terminal-first control plane for Codex that helps one operator supervise a master session plus multiple worker sessions across the same repository.

The 0.13.x line improved multi-session transparency and operator control:

- authoritative local monitor answers for runtime and session visibility
- onboard `Codex Sessions` and `Automations` panels
- bounded `automation create|list|pause|resume|cancel`
- foreground scheduler ticks while `codeclaw up` is open
- stronger slash-command UX, completion, cursor movement, and multiline input
- expanded `inspect --service` runtime heartbeat visibility

v0.13.1 does not change the runtime scope from v0.13.0. It finalizes the tagged release package so the repository snapshot now also includes the release announcement, upgrade notes, and synchronized delivery references.

Release: https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1
Repo: https://github.com/nikkofu/codeclaw
```

## Short Social Post

```md
CodeClaw v0.13.1 is live.

Terminal-first Codex supervision with:
- local runtime/session monitoring
- onboard `Codex Sessions` + `Automations`
- bounded session automation
- scheduler ticks while `up` is open

v0.13.1 freezes the full 0.13.x delivery package into a clean tagged release.

Release: https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1
Repo: https://github.com/nikkofu/codeclaw
```

## LinkedIn Style Post

```md
We shipped CodeClaw v0.13.1.

CodeClaw is our terminal-first control plane for supervising Codex across one master session and multiple worker sessions in the same repository.

The 0.13.x release line focused on operator transparency and control:
- local runtime and session monitoring
- onboard visibility for live sessions and bounded automations
- stronger TUI command-bar UX
- scheduler-driven continuation while the operator console stays open

v0.13.1 is a patch release that freezes the full delivery package into the tagged snapshot, including release announcement and upgrade guidance.

Release: https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1
Repository: https://github.com/nikkofu/codeclaw
```

## IM Group Announcement

```md
CodeClaw v0.13.1 has been released.

Highlights:
- local monitor visibility for runtime and sessions
- onboard `Codex Sessions` and `Automations`
- bounded automation controls
- `up` can keep scheduler work moving
- complete release package now included in the tagged snapshot

Release: https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1
Repo: https://github.com/nikkofu/codeclaw
```

## Email Summary

Subject:

```text
CodeClaw v0.13.1 released
```

Body:

```text
CodeClaw v0.13.1 is now available.

This patch release finalizes the 0.13.x delivery package. The runtime scope remains the same as v0.13.0, including local monitor visibility, onboard session and automation panels, bounded session automation, and foreground scheduler ticks in the TUI.

The main change in v0.13.1 is packaging and handoff completeness: the tagged release now includes the release announcement, upgrade notes, and synchronized delivery references.

Release: https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1
Repository: https://github.com/nikkofu/codeclaw
```

## Reusable CTA Lines

- Explore the release: `https://github.com/nikkofu/codeclaw/releases/tag/v0.13.1`
- Review the repository: `https://github.com/nikkofu/codeclaw`
- Start with the user guide: `docs/user-guide.md`
- Review upgrade guidance: `docs/upgrade-notes-v0.13.1.md`

## Internal Notes

- do not describe `v0.13.1` as a new runtime feature release
- describe it as the finalized tagged package for the `0.13.x` monitor-and-automation line
- if a post needs deeper feature detail, reference `v0.13.0` runtime capabilities and `v0.13.1` packaging completeness
