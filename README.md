# RemoteHub

Gerenciador de conexões remotas Windows com Rust + Tauri 2.

Protótipo 0.0.1: catálogo SQLite, abas e SSH usando OpenSSH do Windows. RDP está representado na interface mas **a conexão RDP ainda não funciona**. Não usar para produção.

```powershell
npm install
npm run tauri dev
```

Requisitos: Windows 10/11, Rust MSVC, Node 22, WebView2, OpenSSH Client. Licença Apache 2.0 (ver LICENSE).
