# Fixed WebRTC UDP Mux Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route server WebRTC UDP media through one configured UDP mux port instead of host ephemeral UDP ports.

**Architecture:** Add a small media config section that defaults to UDP port `40000` and thread it from application settings into `MediaController`. Production media initialization binds one UDP socket, wraps it in `webrtc-rs` UDP mux, and keeps the existing PeerConnection/session model intact; Docker docs switch from an ephemeral range requirement to the configured fixed UDP port.

**Tech Stack:** Rust 2024, Serde YAML, Tokio UDP socket support through `webrtc-rs`, WebRTC UDP mux, Docker Compose, Nginx.

---

## File Structure

- Modify `src/config/settings.rs` and `application.yaml` for `media.udp_mux_port`.
- Modify `src/state.rs` and `src/media/mod.rs` to construct the media controller with UDP mux in production.
- Modify `README.md` to document fixed UDP firewall and troubleshooting behavior.
- Keep VNet tests in `src/media/mod.rs` on their existing injected network path.

### Task 1: Fixed UDP Configuration

- [ ] Add a failing config test in `src/config/settings.rs`:

```rust
assert_eq!(settings.media.udp_mux_port, 40000);
assert!(settings.to_string().contains("media.udp_mux_port = 41000"));
```

- [ ] Run `cargo test config::settings::tests --lib` and confirm it fails because `Settings` has no `media` field.
- [ ] Add `MediaSettings`, its default `udp_mux_port`, settings display output, and `application.yaml` example config.
- [ ] Run `cargo test config::settings::tests --lib` and confirm it passes.

### Task 2: UDP Mux Media Initialization

- [ ] Add a failing media test in `src/media/mod.rs` that creates a controller with a bound mux port and checks the first server ICE candidate port matches that port.
- [ ] Run the focused media test and confirm it fails because the controller has no UDP mux test constructor.
- [ ] Add the UDP mux constructor path with `UDPMuxDefault`, `UDPMuxParams`, `UDPNetwork::Muxed`, and `SettingEngine::set_udp_network`.
- [ ] Thread `settings.media.udp_mux_port` through `AppState::from_settings`.
- [ ] Run the focused media test and the media unit test set.

### Task 3: Deployment Documentation

- [ ] Replace README ephemeral UDP range guidance with fixed mux port guidance and update media troubleshooting bullets.
- [ ] Run `docker compose config` to confirm deployment files still render.
- [ ] Run `git diff --check` to catch documentation whitespace issues.

### Task 4: Final Verification

- [ ] Run `cargo fmt`.
- [ ] Run `cargo test`.
- [ ] Run `node --test tests/frontend/*.test.mjs` because browser room behavior must stay green.
- [ ] Rebuild or restart the Docker stack when available and inspect emitted ICE candidates during local HTTPS flow to confirm the configured UDP port is present.
