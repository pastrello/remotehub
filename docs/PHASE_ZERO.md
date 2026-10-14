# Fase Zero — critérios de aceitação

- [ ] Build frontend `npm run build` e Rust `cargo check` no Windows Actions.
- [ ] CRUD persistente para perfis SSH e RDP via SQLite.
- [ ] Terminal SSH interativo em aba usando PTY Windows/OpenSSH.
- [ ] Revisar host key e autenticação SSH sem armazenar senhas.
- [ ] Duas sessões SSH independentes com ciclo de vida correto.
- [ ] Integrar IronRDP em aba real (não implementado na 0.0.1).
- [ ] Testar NLA, teclado, mouse e reconexão RDP.
- [ ] Auditar encerramento de processos e logs.

RDP é o principal risco técnico. O MVP não está homologado até a validação real do RDP integrado. Nenhuma credencial deve ser persistida em claro.
