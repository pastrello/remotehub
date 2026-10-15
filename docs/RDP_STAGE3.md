# RemoteHub — Etapa RDP 0.0.3: diagnóstico de sessão

## Problema identificado

A interface mostrava apenas **Sessão RDP encerrada**, mesmo quando o IronRDP retornava falha de conexão ou autenticação. O painel mantinha o botão **Desconectar RDP** após o motor ter encerrado a sessão.

O screenshot de laboratório **não valida autenticação**: não houve confirmação de `LoginComplete`.

## Ajustes

- Eventos `rdp-status` agora incluem `eventId`, `phase`, `status` e `terminal`; o identificador de tentativa correlaciona eventos mesmo se chegarem antes da resposta IPC de `rdp_start`.
- Motivo de término é preservado em evento final. Não expor a cadeia de erros interna ou credenciais no WebView.
- Estados: connecting, activated, authenticated, first_image, failed, incomplete, closed.
- Tela volta ao formulário após evento terminal e permite nova tentativa.
- Sem alteração no SSH, sem desabilitar `CertificateValidation::Strict`.
- O motor **ainda não envia pixels para o canvas**. Este é um marco de diagnóstico, não um visualizador completo.

## Homologação sugerida

1. `git pull origin develop`
2. `cargo check --manifest-path src-tauri/Cargo.toml`
3. `cargo test --manifest-path src-tauri/Cargo.toml`
4. `npm run tauri dev`
5. Validar SSH em duas abas e abertura/fechamento.
6. Para um RDP de laboratório, iniciar conexão e registrar **Estado** e **mensagem final**. Se a sessão terminar, o botão **Conectar com IronRDP** deve voltar.
7. Não tratar `RDP ativado` como autenticação; somente `Login RDP confirmado pelo servidor` confirma o evento de logon.
8. Testar credenciais inválidas e certificado não confiável sem instalar exceção TLS nem incluir senhas em logs.

A conexão por endereço IP com certificado autoassinado normalmente falhará na validação TLS estrita. Será necessário nome de servidor e certificado confiável, ou implementar em versão futura um mecanismo de trust-on-first-use/pinning explícito e seguro.
