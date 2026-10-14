# RemoteHub 0.0.2-dev — RDP Stage 1

This milestone introduces a **TCP preflight** for registered RDP profiles. It creates a diagnostic tab and attempts TCP connection to the host and port (default 3389). This does **not** establish an RDP session. A reachable port proves only TCP connectivity, not RDP availability, security or credentials.

## Next implementation stage — IronRDP

- Dedicated Rust async session actor for each RDP tab.
- IronRDP TLS and NLA/CredSSP negotiation, with verified certificate handling (no silent acceptance).
- Credentials memory-only; never persist, log or pass to URLs.
- Bounded framebuffer pipeline to graphical Tauri tab.
- Keyboard, mouse, resize and session teardown.
- Test two SSH plus one RDP on Windows 10/11, then concurrent RDP sessions.

References: https://github.com/Devolutions/IronRDP and https://docs.rs/ironrdp-client/

## Manual acceptance for Stage 1

1. Existing SSH connections and tabs work as before.
2. RDP profile opens a diagnostic tab displaying TCP availability.
3. Unreachable TCP host reports failure within bounded time.
4. No RDP credentials are solicited or persisted.
5. User sees explicit notice that graphics/NLA are not implemented.

**Not yet validated on Windows:** Build, RDP preflight and SSH regression. Please run `git pull origin develop`, `npm run build`, `cargo check --manifest-path src-tauri/Cargo.toml` and `npm run tauri dev`.

## Stage 2 — X.224 RDP security negotiation (2026-10-14)

Implemented in `src-tauri/src/rdp.rs` with command `rdp_negotiate`.
Sends one RDP_NEG_REQ (TLS, HYBRID, HYBRID_EX), validates bounded TPKT/X.224 confirmation and decodes RDP_NEG_RSP or failure. No credentials are transmitted; the connection closes after the response. Three Rust unit tests cover NLA response, negotiation failure and malformed PDU.

**Not implemented:** TLS handshake, certificate verification, CredSSP/NLA exchange, RDP authentication, graphics, mouse or keyboard.

Manual test on Windows:

```cmd
cd /d C:\software\RemoteHub\remotehub
git pull origin develop
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run tauri dev
```

Create an RDP profile for a lab Windows Server with RDP enabled and observe an X.224 result such as `CredSSP/NLA selecionado`. An accessible port alone is NOT an authenticated RDP session. Keep the SSH regression test with two tabs.

Future task: integrate IronRDP 0.17 session/connector and a bounded pixel pipeline into the tab. Do not enable an insecure certificate bypass or hardcode credentials.
