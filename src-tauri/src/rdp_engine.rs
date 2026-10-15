//! IronRDP session lifecycle with structured, non-secret status events.
//! No framebuffer is sent to the WebView yet; strict TLS verification is mandatory.
use ironrdp::client::{
    config::{ConfigBuilder, Destination},
    output_channel::output_channel,
    rdp::{RdpClient, RdpInputSender, RdpOutputEvent},
};
use ironrdp::connector::ConnectorErrorKind;
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp_tls::CertificateValidation;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{atomic::{AtomicBool, Ordering}, Arc},
};
use tauri::{AppHandle, Emitter};

struct SessionHandle {
    sender: RdpInputSender,
    close_requested: Arc<AtomicBool>,
}
pub struct Engine {
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
}
#[derive(Default)]
struct SessionOutcome {
    activated: bool,
    login_confirmed: bool,
    first_image: bool,
    failure: Option<&'static str>,
}

fn failure_label(kind: &ConnectorErrorKind) -> &'static str {
    // Never serialize external error chains or the user's password into webview events.
    match kind {
        ConnectorErrorKind::AccessDenied => "Acesso negado pelo servidor RDP",
        ConnectorErrorKind::Credssp(_) => "Falha na autenticação CredSSP/NLA",
        ConnectorErrorKind::Negotiation(_) => "Servidor recusou a negociação RDP",
        _ => "Falha RDP antes da confirmação do login (verifique certificado TLS, rede e credenciais)",
    }
}
fn notify(app: &AppHandle, event_id: &str, phase: &str, status: &str, terminal: bool) {
    let _ = app.emit("rdp-status", serde_json::json!({
        "eventId": event_id, "phase": phase, "status": status, "terminal": terminal
    }));
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
        event_id: String,
    ) -> Result<String, String> {
        if uuid::Uuid::parse_str(&event_id).is_err() {
            return Err("Identificador de aba RDP inválido".into());
        }
        if username.trim().is_empty() || password.is_empty() {
            return Err("Usuário e senha são obrigatórios".into());
        }
        if port == 0 || host.trim().is_empty()
            || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("Destino RDP inválido".into());
        }
        let address = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let destination = Destination::new(address).map_err(|_| "Destino RDP inválido")?;
        // Strict is NOT IronRDP's default. This is a deliberate security policy.
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
            .map_err(|_| "Falha na configuração do cliente RDP")?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let (output_tx, mut output_rx) = output_channel(16);
        let client = RdpClient::new(config, output_tx);
        let sender = client.input_sender();
        let close_requested = Arc::new(AtomicBool::new(false));
        self.sessions.lock().insert(session_id.clone(), SessionHandle {
            sender,
            close_requested: close_requested.clone(),
        });

        let sessions = Arc::clone(&self.sessions);
        let sid = session_id.clone();
        let spawn_result = std::thread::Builder::new()
            .name("remotehub-rdp".into())
            .spawn(move || {
                notify(&app, &event_id, "connecting", "Iniciando autenticação RDP com certificado TLS verificado", false);
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                let outcome = match rt {
                    Ok(rt) => rt.block_on(async {
                        // RdpClient::run() is !Send in this pinned IronRDP revision.
                        // Drive it and its bounded event channel on the same thread.
                        let receive = async {
                            let mut outcome = SessionOutcome::default();
                            while let Some(event) = output_rx.recv().await {
                                match event {
                                    RdpOutputEvent::Connected => {
                                        outcome.activated = true;
                                        notify(&app,&event_id,"activated","Conexão RDP ativada; aguardando confirmação de login",false);
                                    }
                                    RdpOutputEvent::LoginComplete => {
                                        outcome.login_confirmed = true;
                                        notify(&app,&event_id,"authenticated","Login RDP confirmado pelo servidor",false);
                                    }
                                    RdpOutputEvent::ConnectionFailure(err) => {
                                        let label = failure_label(err.kind());
                                        outcome.failure = Some(label);
                                        notify(&app,&event_id,"failed",label,false);
                                    }
                                    RdpOutputEvent::Terminated(result) => {
                                        if result.is_err() && outcome.failure.is_none() {
                                            outcome.failure = Some("Transporte ou sessão RDP encerrado com erro");
                                        }
                                    }
                                    RdpOutputEvent::Image { .. } if !outcome.first_image => {
                                        outcome.first_image = true;
                                        notify(&app,&event_id,"first_image","Primeiro frame recebido; renderização ainda não implementada",false);
                                    }
                                    _ => {}
                                }
                            }
                            outcome
                        };
                        let (_, outcome) = tokio::join!(client.run(), receive);
                        outcome
                    }),
                    Err(_) => SessionOutcome {
                        failure: Some("Falha ao inicializar o runtime RDP"),
                        ..SessionOutcome::default()
                    },
                };
                let was_closed = close_requested.load(Ordering::Acquire);
                let (phase, message) = if was_closed {
                    ("closed", "Sessão encerrada a pedido do usuário".to_owned())
                } else if let Some(failure) = outcome.failure {
                    ("failed", failure.to_owned())
                } else if outcome.login_confirmed {
                    ("closed", "A sessão RDP autenticada foi encerrada".to_owned())
                } else if outcome.activated {
                    ("incomplete", "RDP ativado, mas encerrado antes da confirmação de login".to_owned())
                } else {
                    ("incomplete", "RDP encerrado antes da ativação; possível problema TLS ou de autenticação".to_owned())
                };
                // Terminal event retains the failure reason instead of overwriting it.
                notify(&app,&event_id,phase,&message,true);
                sessions.lock().remove(&sid);
            });
        if spawn_result.is_err() {
            self.sessions.lock().remove(&session_id);
            return Err("Falha ao iniciar o processamento da sessão RDP".into());
        }
        Ok(session_id)
    }

    pub fn close(&self, id: &str) {
        if let Some(handle) = self.sessions.lock().get(id) {
            handle.close_requested.store(true, Ordering::Release);
            handle.sender.request_close();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_classification_never_echoes_passwords() {
        assert_eq!(failure_label(&ConnectorErrorKind::AccessDenied),"Acesso negado pelo servidor RDP");
        assert_eq!(failure_label(&ConnectorErrorKind::General),"Falha RDP antes da confirmação do login (verifique certificado TLS, rede e credenciais)");
    }
}
