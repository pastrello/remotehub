# RemoteHub 0.0.4 — First embedded RDP graphics (experimental)

## Delivered

- IronRDP `RdpOutputEvent::Image` framebuffer is encoded on a **separate worker thread** to JPEG (quality 76); a bounded sync queue holds at most one unprocessed frame. Frames arriving while the worker is busy are dropped, rather than blocking the async RDP loop.
- A maximum of 12 frames/second is sent. Resolution is capped at 2560×1600 for this prototype, with strict pixel-buffer length validation.
- Frames are emitted to Tauri as `rdp-frame` (base64 JPEG, session/attempt ID, sequence number, width/height). Frames are not persisted.
- A canvas in each RDP tab decodes the JPEG with `createImageBitmap`, renders it to its native resolution and scales the canvas to fit the workspace. Old/out-of-order frames are discarded.
- Basic left/right/middle mouse button, pointer movement, vertical wheel, US physical-keyboard scancode mapping and key up/down are sent by `rdp_input` to bounded IronRDP `FastPath` input.
- Keyboard input is focused only after a click on the canvas. Keys held when focus is lost are released. Each RDP session has its own canvas and event token.
- Existing SSH adapter and connection database are unchanged.
- TLS strict validation remains the default; the previously approved lab-only per-attempt certificate exception is retained.

## Known limitations

1. Proof of concept, **not production-ready**. Frames over 2560×1600 are dropped. Large bitmaps consume memory; FPS target depends on CPU and host/RDP encoding. Neither 12 FPS nor low latency is guaranteed.
2. Frame transmission uses base64 JPEG on Tauri's event bus, not a zero-copy shared-memory transport. Future optimized design should use a binary IPC stream, dirty rectangles or a GPU-friendly surface.
3. No clipboard, audio, multi-monitor, IME/Unicode text, layout-specific scan codes, dynamic server resize, lock-key synchronization or advanced cursor images yet. Brazilian ABNT2 requires additional mapping tests.
4. Browser/Windows may intercept particular reserved combinations (including Ctrl+Alt+Delete).
5. Screen JPEG is lossy. Clipboard and text rendering should be evaluated at normal and high DPI.
6. Self-signed certificates should use explicit acceptance per attempt only in trusted labs. Use pinned/approved certs for repeated connections.

## Windows acceptance

```cmd
cd /d C:\software\RemoteHub\remotehub
git pull origin develop
npm install
npm run build
cargo check --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml
npm run tauri dev
```

Test with lab Windows 11 profile:

- Verify password + optional TLS certificate consent flow reaches `LoginComplete`.
- Expect a desktop image in canvas and the received resolution in toolbar.
- Click canvas, move mouse, click buttons, scroll, use alphanumeric keys, Backspace, Enter, arrows and Alt+Tab (some host OS shortcuts cannot be captured).
- Open 2 SSH tabs, then the RDP tab; switch back and forth without disconnecting.
- Minimize/restore app and close RDP tab; verify connection closes cleanly.
- Watch CPU/memory for five minutes; document responsiveness and any frame artifacts.

If there is no image but `LoginComplete` appears, capture the **visible status text** and the Rust output (redact credentials), not a production screenshot with sensitive content.
