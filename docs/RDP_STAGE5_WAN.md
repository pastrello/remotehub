# RemoteHub 0.0.5-dev — WAN viability gate

## Why this cycle exists

The Windows 11 lab established that the RemoteHub 0.0.4 canvas displayed a valid desktop but had delayed keyboard/mouse response and blank/late context menus. **About 99% of intended use is over WAN**, so a fast LAN demo is not enough to justify continuation.

## Technical changes

- Enables `RdpClient::with_desktop_updates()`, using ordered `DesktopUpdate` dirty regions.
- A Rust framebuffer compositor applies **every** region in order; the presentation bridge coalesces the currently dirty area without losing the backing framebuffer.
- At most one JPEG rectangle is outstanding before the frontend acknowledges it after successful canvas painting via `rdp_frame_ack`.
- If an ACK is missing for more than 1.5 seconds, Rust forces a complete framebuffer refresh; it never silently treats skipped delta updates as correct.
- JPEG quality 78; target maximum 25 presentation updates per second. Both are starting parameters, not promised performance.
- Frame encoder runs off the RDP actor thread; its queue remains bounded to one frame.
- Frontend draws patches at their original canvas coordinates. It does not discard a dirty patch merely because the tab is hidden.
- Ordinary mouse moves remain disposable. Key and button transitions are serialized and retried up to three times; the UI counts failed inputs.
- Footer shows **canvas FPS**, JPEG bridge throughput (KiB/s), mean Rust-to-WebView/paint latency (ms), and compression time (ms). These measurements **do not represent WAN RTT or RDP protocol bandwidth**.
- SSH implementation, saved profiles, password policy and explicit certificate exception remain unchanged.

## Validation — Windows lab

```cmd
cd /d C:\software\RemoteHub\remotehub
git pull origin develop
npm run build
cargo test --manifest-path src-tauri\Cargo.toml
npm run tauri dev
```

Compare against Microsoft `mstsc.exe`, on **the same PC, network and remote Windows host**, with comparable resolution and without running both clients simultaneously during the measurement.

Test A: LAN, 1280x720. Open/close Windows context menus 20 times, move windows continuously for 30 seconds, type 100+ characters in Notepad, hold Backspace, scroll a long page, switch between two SSH tabs and RDP. **All menus must fully draw and no keys/buttons must remain stuck.**

Test B: realistic WAN over the existing authorized VPN: repeat at low, medium and high network latency if available (examples 80 / 150 / 200 ms RTT), noting round-trip delay using an independent ping command and network conditions. Record:
- Average and worst perceived time to see keypress echo and context menu;
- Approximate visual freshness vs MSTSC; 
- RemoteHub FPS and local bridge/compression latency from footer;
- CPU and working-set memory during 5 minutes;
- Corrupted/stale rectangles, dropped keyboard transitions, disconnects.

## Decision gate

**Continue** if image corruption disappears and perceived WAN interactions are close enough to MSTSC to perform routine administration (initial experimental target: no consistent extra 100 ms of client-side latency), with stable sessions and acceptable resource use.

**Rework** the graphics transport if JPEG/JSON bridge time or drawing queue is materially responsible for response delay; next options are binary IPC, region codecs and GPU upload.

**Stop or change RDP embedding strategy** if, after those bounded experiments, the difference versus MSTSC remains operationally unacceptable. Do not expand into VNC or UI polish until the gate passes.

## Current technical limits

- Cap 2560x1600, initial JPEG delta transport and no multi-monitor/advanced cursor.
- Keyboard is set-1 scan-code based; ABNT2 and IME need a later dedicated test.
- Clipboard/audio/drive redirection and dynamic server resizing not part of this cycle.
- The current certificate bypass explicitly accepts *any* identity for the one attempt. For routine connections prefer certificate pinning (future).
- Metrics are approximate and intended for comparative diagnosis, not formal benchmarking.
