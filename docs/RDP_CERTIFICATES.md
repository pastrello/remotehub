# RDP certificates — RemoteHub phase zero

The IronRDP client starts with **Strict TLS certificate validation** on every connection.

## Lab exception
RDP tabs offer a manually selected checkbox **Aceitar certificado RDP inválido (somente nesta tentativa)**. The checkbox:
- defaults to unchecked on every new tab;
- is not stored in SQLite or reused for future connections;
- triggers an explicit user confirmation naming the destination before connecting;
- selects `CertificateValidation::DangerouslyAcceptInvalidCertificate` in IronRDP for that one attempt only.

**Security warning:** the exception disables server identity verification. A person controlling the network path can impersonate the destination and potentially intercept credentials. Prefer strict validation with trusted host certificates where possible. Never enable this automatically or on a global basis.

Accepting an invalid certificate does not establish authentication. A connection is only marked as logged on after IronRDP emits `LoginComplete`.

## Next security improvement
Certificate fingerprint display and per-host public-key/certificate pinning, confirmed on first use with a warning when changed. This is safer for ongoing private RDP server administration than repeated unverified connections.

## Windows verification
```cmd
git pull origin develop
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```
Open a lab RDP session. With the box unchecked, self-signed certificates should continue to fail. With it checked and confirmed, negotiation should proceed past TLS (though NLA may fail for other reasons). Retest SSH and concurrent tabs.

Never paste RDP passwords into GitHub issues or logs.
