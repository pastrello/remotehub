# RemoteHub 0.0.3-dev — IronRDP authentication experiment

**Experimental: not production ready.** The tested SSH PTY adapter remains unchanged.

## Implemented
- Separate `rdp_engine.rs` session actor, using `ironrdp 0.17` reusable client engine, rustls, and Tokio.
- Credentials are provided by the user at connect time, passed via local Tauri IPC, and are **not stored in SQLite**. They may remain in process memory until freed.
- Certificate validation is explicitly `CertificateValidation::Strict`. This rejects a self-signed or otherwise untrusted lab certificate; do not disable verification.
- RDP tab now includes a password field, start and close actions, and subscribes to connection status events.
- `Connected`, `LoginComplete`, `ConnectionFailure`, and first `Image` are recognized. **Framebuffer pixels are intentionally not transported or rendered** in this build.
- Existing X.224 probe remains available and shows the selected security protocol.

## Critical limitations
- **Not yet compiled or tested on Windows**; this is the first IronRDP integration. The new dependency tree may need adjustments.
- Authenticating in a live network requires a trusted TLS certificate with the server name matching the name used in the profile. Connecting by IP requires the certificate to contain a matching IP subjectAltName.
- The X.224 preflight is an independent connection. It is not an authenticated session and must never be treated as one.
- No credential vault, clipboard, keyboard/mouse forwarding, display rendering, or PIN/certificate management.
- Errors visible in the UI are intentionally generic to avoid exposing protocol-internal sensitive data; diagnostic logs must be reviewed before enabling detailed tracing.

## Windows test
Use **laboratory** credentials only.

```cmd
cd /d C:\software\RemoteHub\remotehub
git pull origin develop
cargo check --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run tauri dev
```

For a Windows target with a trusted RDP certificate, set a username in the stored RDP profile, open its RDP tab, type the password, and choose **Conectar com IronRDP**. The UI should report TLS/NLA status, or reject invalid certificates. A status indicating `LoginComplete` is required before describing authentication as validated.

## Follow-up acceptance
1. Build and existing SSH regression on Windows.
2. Trusted RDP server: NLA authentication / LoginComplete event.
3. Invalid server certificate: authentication blocked, no exception switch.
4. Network failure: no process leak, close button stops actor.
5. Only after acceptance: bounded RDP frame stream, image decoding on WebView, then input forwarding.
