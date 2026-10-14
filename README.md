# RemoteHub

Gerenciador de conexões remotas para Windows, em Rust + Tauri 2.

## Fase Zero

Esta é uma prova de conceito **experimental**. Objetivos: perfis locais de SSH/RDP, abas e terminal SSH; o motor RDP integrado em abas (IronRDP) **ainda não está implementado**.

- Frontend: React + TypeScript + xterm.js
- Backend: Rust/Tauri
- Persistência: SQLite, sem armazenamento de senhas
- SSH PoC: cliente `ssh.exe` local, terminal PTY; backend Rust `russh` ainda não integrado

## Executar (Windows)

Requer Node.js 22+, Rust MSVC, WebView2, ferramentas de build do Tauri e OpenSSH Client.

```powershell
npm install
npm run tauri dev
```

Para compilar: `npm run build` e `cargo check --manifest-path src-tauri/Cargo.toml`.

Consulte `docs/PHASE_ZERO.md`. Nunca use credenciais de produção nesta PoC.

Licença: Apache-2.0.
