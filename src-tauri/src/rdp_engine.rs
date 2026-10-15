//! IronRDP runtime: authenticated RDP session lifecycle.
//! Important: the frontend must never persist passwords and the certificate
//! policy here MUST remain Strict. Framebuffer transport is a separate milestone.
use ironrdp::client::{
    config::{ConfigBuilder, Destination},
    output_channel::output_channel,
    rdp::{RdpClient, RdpInputSender, RdpOutputEvent},
};
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp_tls::CertificateValidation;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};
use tauri::{AppHandle, Emitter};

pub struct Engine {
    sessions: Arc<Mutex<HashMap<String, RdpInputSender>>>,
}

impl Engine {
    pub fn new() -> Self {
        Self { sessions: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn start(
        &self,
        app: AppHandle,
        host: String,
        port: u16,
        username: String,
        password: String,
    ) -> Result<String, String> {
        if username.trim().is_empty() || password.is_empty() {
            return Err("Usuário e senha são obrigatórios".to_owned());
        }
        let destination = Destination::new(format!("{host}:{port}"))
            .map_err(|_| "Destino RDP inválido".to_owned())?;
        // Strict is NOT the library default. Never disable it silently.
        let config = ConfigBuilder::new()
            .with_destination(destination)
            .with_username(username)
            .with_password(password)
            .with_client_build(26000)
            .with_client_dir("C:\\Windows\\System32\\mstscax.dll")
            .with_client_name("RemoteHub")
            .with_platform(MajorPlatformType::WINDOWS)
            .with_certificate_validation(CertificateValidation::Strict)
            .build()
            .map_err(|_| "Não foi possível configurar a sessão RDP".to_owned())?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let (output_tx, mut output_rx) = output_channel(16);
        let client = RdpClient::new(config, output_tx);
        let sender = client.input_sender();
        self.sessions.lock().insert(session_id.clone(), sender);
        let sessions = Arc::clone(&self.sessions);
        let sid = session_id.clone();
        let thread_start = std::thread::Builder::new()
            .name("remotehub-rdp".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all().build();
                match rt {
                    Ok(rt) => rt.block_on(async {
                        // IronRDP's run() future is !Send in this revision.
                        // Poll it and the event consumer concurrently on the SAME runtime
                        // thread; do not use tokio::spawn() (which requires Send).
                        let events = async {
                            let mut seen_image = false;
                            while let Some(event) = output_rx.recv().await {
                                let status = match event {
                                    RdpOutputEvent::Connected => Some("RDP conectado (ativação da sessão)"),
                                    RdpOutputEvent::LoginComplete => Some("Login RDP confirmado pelo servidor"),
                                    RdpOutputEvent::ConnectionFailure(_) => Some("Falha na negociação ou autenticação RDP; confira certificado e credenciais"),
                                    RdpOutputEvent::Image { .. } if !seen_image => {
                                        seen_image = true;
                                        Some("Frame RDP recebido; renderizador em desenvolvimento")
                                    }
                                    _ => None
                                };
                                if let Some(status) = status {
                                    let _ = app.emit("rdp-status", serde_json::json!({"id":sid,"status":status}));
                                }
                            }
                        };
                        tokio::join!(client.run(), events);
                    }),
                    Err(_) => {
                        let _ = app.emit("rdp-status", serde_json::json!({"id":sid,"status":"Falha ao iniciar runtime RDP"}));
                    }
                }
                sessions.lock().remove(&sid);
                let _ = app.emit("rdp-status", serde_json::json!({"id":sid,"status":"Sessão RDP encerrada"}));
            });
        if thread_start.is_err() {
            self.sessions.lock().remove(&session_id);
            return Err("Falha ao iniciar thread RDP".into());
        }
        Ok(session_id)
    }

    pub fn close(&self, id: &str) {
        if let Some(sender) = self.sessions.lock().remove(id) {
            sender.request_close();
        }
    }
}
