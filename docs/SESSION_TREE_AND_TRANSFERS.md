# RemoteHub 0.0.6-dev — Session tree + FTP / SFTP / SCP

## Scope

RDP development is frozen at 0.0.5 (no new RDP features). The existing SSH PTY adapter is preserved. This build adds:

- MobaXterm-inspired nested folders to the left sidebar.
- Context menu on a **profile**: Executar, Transferir arquivos (where supported), Editar, Clonar, Apagar.
- Context menu on a **folder**: Nova conexão, Nova subpasta, Renomear pasta, Excluir pasta vazia.
- Profiles SSH, RDP, SFTP, SCP, FTP.
- Native browsing and file transfer (upload/download) in a dedicated tab.
- An SSH profile can open a separate SFTP file manager tab without duplicating the profile.
- Passwords are entered per session only, not saved in SQLite.
- FTP plaintext access is disabled until explicitly consented to at login.
- File transfers stream to/from disk, with transfer-progress events. Local downloads are written to a temporary file before moving into place. Overwrites are disabled by default and explicitly confirmed.

## Security and compatibility

- SSH file transfers use libssh2 and the user's OpenSSH `~/.ssh/known_hosts`. Mismatches and unrecognized host keys are rejected. For first-time hosts, establish an ordinary interactive SSH connection (whose host-key prompt is user-visible) before opening SFTP or SCP.
- SSH auth: a filled password uses password auth; with the field blank, try ssh-agent or one of the standard unencrypted private keys (id_ed25519, id_ecdsa, id_rsa).
- SCP uses the SSH SCP channel for legacy servers without SFTP. Without the SFTP subsystem, listing and remote directory operations are not available: enter full remote paths. For uploading to a legacy SCP server, explicit overwrite permission is required because SCP lacks portable atomic exclusivity.
- FTP is plaintext, including user credentials. Prefer SFTP; FTPS is **not yet supported**. FTP listings can vary between server implementations, and unsupported formats might not display all files; direct-path downloads/uploads are still available.
- SQLite migration copies records inside a transaction to a new CHECK constraint including ftp/sftp/scp. IDs, existing folder names and saved profiles remain intact.
- Rename folder updates nested folder paths and profile locations transactionally. Deletion of nonempty folders is blocked.

## Windows manual acceptance

```cmd
cd /d C:\software\RemoteHub\remotehub
git pull origin develop
npm run build
cargo test --manifest-path src-tauri\Cargo.toml
npm run tauri dev
```

1. Existing SSH profiles and SSH multi-tabs still work. Existing RDP profiles are still present and launch unchanged.
2. Create folder with child folder. Move/create SSH profile in child. Right-click profile, execute/edit/clone/delete. Rename parent; child paths and profiles must follow.
3. Open SFTP using existing SSH profile. If host key is not known, open that host once in terminal SSH and approve its key. Open file tab again, authenticate with agent/private key or password. List `/` and a remote folder, upload a small test file, download it, and verify SHA-256 hashes match.
4. Repeat SCP upload/download, test both a server with SFTP and one with only legacy SCP if available.
5. Test FTP on an authorized lab FTP server, making explicit consent to plaintext credentials. List, upload, download and compare contents.
6. Try to delete nonempty folder; operation must refuse. Try changing the remote host key; session must be blocked. Test large file transfers and confirm the UI shows progress.
7. Exit and restart; session tree, folders and profiles must persist.

## Known limitations

- Directory synchronization, recursive uploads/downloads, file editing, permission changes, resume, transfer queue, and FTPS are postponed until basic WAN workflows are validated.
- SCP without SFTP uses full remote paths (no remote directory browser).
- File operations against a given transfer session are serialized. Other sessions remain independent.
- Initial transfer implementation uses blocking I/O on Tauri commands. Large-transfer responsiveness and interruption behavior require performance testing. Closing a session will not necessarily cancel an in-progress file transfer.
- Authentication is not yet integrated with the SSH terminal session: SFTP establishes a separate SSH connection, and an agent or password is required again.
