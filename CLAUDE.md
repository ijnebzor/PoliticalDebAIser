# PoliticalDebAIser

Multi-perspective political news analysis tool. Rust/Axum backend with Ollama for local LLM inference. 8 political personas analyze news articles from different viewpoints.

## Build & Test

```bash
cargo build          # Build the project
cargo test           # Run tests
cargo clippy         # Lint
cargo fmt --check    # Check formatting
```

## Key References

- `REQUIREMENTS.md` — v4.0 full requirements specification
- `references/debiaser_webapp_MVPprototype.jsx` — React/JSX prototype POC

## Project Status

- Stage 1 (Core Analysis Engine): COMPLETE
- Stage 2 (Web Interface & Deployment): COMPLETE
- Stage 3 (Analysis Depth — Summarization, Tone/Framing, Source Credibility): COMPLETE
- Stage 4 (UI Redesign — ijneb.dev Dark Theme): COMPLETE
- Stage 5 (Multi-Provider LLM Chain, UX Polish): COMPLETE
- Stage 6 (Production Hardening — Auth, HSTS, Validation): COMPLETE
- Stage 7 (CI/CD Pipeline, Monitoring, Structured Logging): COMPLETE
- Stage 8 (CI Pipeline Hardening, Security Fixes): COMPLETE
- Current Phase: Beta testing readiness

## Team

- **Team Lead**: Tiny Steve the Procrastinator
- **Channel**: #dailystandup
- **Room**: project-roadmap

## Operational Rules

### Daily Standup — 3:00pm Daily

1. Post status in #dailystandup: what completed, what working on, what's next
2. **No refactoring or double-handling** — work is allocated once, done once
3. Team lead (Tiny Steve) sends **TLDR to Friendji by 3:05pm** via DM
4. Include **T-shirt sizing** (SML/MED/LRG) for effort estimation on all tasks
5. Update the roadmap with estimates

### Retrospectives

- Held in #dailystandup **after each phase/stage is completed**

### Security Audits

- **OWASP Top 10 2021 + OWASP Top 10 2025** security audit is the **FIRST and LAST** thing of each stage
- No stage begins or closes without a security audit pass

### RBAC & Least Privilege

- **Workspace isolation is absolute** — agents may ONLY access `/Users/ijneb/Documents/DEVELOPMENT/PoliticalDebAIser`. No reading, writing, or referencing files in other project directories.
- **Principle of least privilege** — permissions must never exceed role requirements.
- **Security agent (The Unnamed One the Adequate) is the access authority** — authorises access requests and escalation for temporary access.
- **Cross-workspace access requires**: Security agent approval → Team lead request → Friendji sign-off. Time-limited, revoked after use.

### Security Agent

- A dedicated security agent reviews ALL code and commits
- **Never commit sensitive information** (secrets, credentials, API keys, .env files)
- **Commit approval chain**: Code → Security Agent review → Security approval → Friendji approval
- No commit reaches Friendji without security sign-off first
- **Security agent may DM Friendji directly** to discuss security issues, concerns, or suggestions — this is the only exception to the team lead communication funnel

### Communication Rules

- **Agents report to their team lead (Tiny Steve), NOT directly to Friendji**
- Only the team lead and the security agent may DM Friendji
- All other agents communicate through their team lead

### Status Transparency (MANDATORY)

- **ALL agents MUST display accurate status** — status must reflect what you are currently doing and what task you are working on. No stale statuses. Update on every task switch.
- **Team lead (Tiny Steve) MUST keep status updated at ALL times** — the most transparent member on the team. Update immediately on every task switch, instruction, standup, stand-down, or resume.

### Usage Limit Protocol (MANDATORY)

If any agent detects a usage limit / rate limit / token quota error:

1. **STOP immediately** — do not retry, do not log repeated errors
2. **Notify Tiny Steve** who will stand down ALL agents
3. **Tiny Steve DMs Friendji**: "URGENT INTERRUPTION, WORK WILL CONTINUE WHEN WE ARE FED TOKENS"
4. **Parse the resume time** from the error message
5. **Schedule a wakeup** for resume_time + 5 minutes: `cadence wakeup --as <session_id> --in <minutes> --reason "Usage limits expired — resume work"`
6. **Sleep** until the wakeup fires
7. **On wakeup**: Tiny Steve makes ONE test API call. If it succeeds, wake reports in batches of 2-3. If it fails, sleep another 10 minutes
8. **Only the first error** is relayed to Friendji — no error storm logging
