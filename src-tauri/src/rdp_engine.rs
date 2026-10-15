//! IronRDP session lifecycle with structured, non-secret status events.
//! Full session with ordered dirty rectangles, compositor, and acknowledged JPEG deltas.
//! An explicit per-attempt opt-in can bypass certificate validation for lab servers.
use ironrdp::client::{
    config::{ConfigBuilder, Destination},
    output_channel::output_channel,
    rdp::{RdpClient, RdpInputEvent, RdpInputSender, RdpOutputEvent},
};
use ironrdp::connector::ConnectorErrorKind;
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp::pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};
use ironrdp::pdu::input::mouse::{MousePdu, PointerFlags};
use serde::Deserialize;
use smallvec::smallvec;
use crate::rdp_frames::{FRAME_INTERVAL_MS, ACK_TIMEOUT_MS};
use crate::rdp_compositor::{Compositor,Rect};
use tokio::sync::mpsc;
use ironrdp_tls::CertificateValidation;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Deserialize)]
#[serde(tag="kind", rename_all="snake_case")]
pub enum InputAction {
    Mouse { action: MouseAction, x: u16, y: u16, button: u8, wheel: i16 },
    Key { scancode: u8, extended: bool, pressed: bool },
}
#[derive(Debug, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum MouseAction { Move, Down, Up, Wheel }

fn to_fastpath(input: InputAction) -> Result<FastPathInputEvent, String> {
    match input {
        InputAction::Mouse {action,x,y,button,wheel} => {
            let flags = match action {
                MouseAction::Move => PointerFlags::MOVE,
                MouseAction::Wheel => PointerFlags::VERTICAL_WHEEL,
                MouseAction::Down | MouseAction::Up => {
                    let button_flag=match button {
                        0 => PointerFlags::LEFT_BUTTON,
                        1 => PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
                        2 => PointerFlags::RIGHT_BUTTON,
                        _ => return Err("Botão do mouse não suportado".into()),
                    };
                    if matches!(action,MouseAction::Down) {button_flag|PointerFlags::DOWN} else {button_flag}
                }
            };
            let rotation=if matches!(action,MouseAction::Wheel) {
                wheel.clamp(-256,255)
            }else{0};
            Ok(FastPathInputEvent::MouseEvent(MousePdu {
                flags,
                number_of_wheel_rotation_units:rotation,
                x_position:x, y_position:y
            }))
        }
        InputAction::Key {scancode,extended,pressed} => {
            if scancode == 0 { return Err("Código de tecla inválido".into()); }
            let mut flags=KeyboardFlags::empty();
            if extended {flags|=KeyboardFlags::EXTENDED;}
            if !pressed {flags|=KeyboardFlags::RELEASE;}
            Ok(FastPathInputEvent::KeyboardEvent(flags,scancode))
        }
    }
}

struct SessionHandle {
    sender: RdpInputSender,
    frame_ack: mpsc::Sender<u64>,
    event_id: String,
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
fn certificate_policy(allow_invalid_certificate: bool) -> CertificateValidation {
    if allow_invalid_certificate {
        CertificateValidation::DangerouslyAcceptInvalidCertificate
    } else {
        CertificateValidation::Strict
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
        allow_invalid_certificate: bool,
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
        // Strict remains the default. Insecure mode requires per-attempt UI consent.
        let config = ConfigBuilder::new()
            .with_destination(destination)
            .with_username(username)
            .with_password(password)
            .with_client_build(26000)
            .with_client_dir("C:\\Windows\\System32\\mstscax.dll")
            .with_client_name("RemoteHub")
            .with_platform(MajorPlatformType::WINDOWS)
            .with_certificate_validation(certificate_policy(allow_invalid_certificate))
            .build()
            .map_err(|_| "Falha na configuração do cliente RDP")?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let (output_tx, mut output_rx) = output_channel(64);
        let client = RdpClient::new(config, output_tx).with_desktop_updates();
        let (frame_ack, mut ack_rx) = mpsc::channel::<u64>(4);
        let sender = client.input_sender();
        let close_requested = Arc::new(AtomicBool::new(false));
        self.sessions.lock().insert(session_id.clone(), SessionHandle {
            sender,frame_ack,event_id:event_id.clone(),
            close_requested: close_requested.clone(),
        });

        let sessions = Arc::clone(&self.sessions);
        let sid = session_id.clone();
        let spawn_result = std::thread::Builder::new()
            .name("remotehub-rdp".into())
            .spawn(move || {
                notify(&app, &event_id, "connecting", if allow_invalid_certificate {
                    "Iniciando RDP com validação do certificado DESATIVADA nesta tentativa"
                } else {
                    "Iniciando autenticação RDP com certificado TLS verificado"
                }, false);
                let (frame_tx, frame_rx) = crate::rdp_frames::bounded_frame_channel();
                let frame_app = app.clone();
                let frame_id = event_id.clone();
                let worker = std::thread::Builder::new()
                    .name("remotehub-rdp-jpeg".into())
                    .spawn(move || crate::rdp_frames::process_frames(frame_app,frame_id,frame_rx));
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                let outcome = match rt {
                    Ok(rt) => rt.block_on(async {
                        // RdpClient::run() is !Send in this pinned IronRDP revision.
                        // Drive it and its bounded event channel on the same thread.
                        let receive = async {
                            let mut outcome=SessionOutcome::default();
                            let mut compositor=Compositor::default();
                            let mut sequence=0_u64;
                            let mut inflight:Option<(u64,Instant)>=None;
                            let mut ticker=tokio::time::interval(Duration::from_millis(FRAME_INTERVAL_MS));
                            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                            loop {
                                tokio::select! {
                                    event = output_rx.recv() => {
                                        let Some(event)=event else {break;};
                                        match event {
                                            RdpOutputEvent::Connected=>{
                                                outcome.activated=true;
                                                notify(&app,&event_id,"activated","Conexão RDP ativada; aguardando confirmação de login",false);
                                            }
                                            RdpOutputEvent::LoginComplete=>{
                                                outcome.login_confirmed=true;
                                                notify(&app,&event_id,"authenticated","Login RDP confirmado pelo servidor",false);
                                            }
                                            RdpOutputEvent::ConnectionFailure(err)=>{
                                                let label=failure_label(err.kind());
                                                outcome.failure=Some(label);
                                                notify(&app,&event_id,"failed",label,false);
                                            }
                                            RdpOutputEvent::Terminated(result)=>{
                                                if result.is_err() && outcome.failure.is_none(){
                                                    outcome.failure=Some("Transporte ou sessão RDP encerrado com erro");
                                                }
                                            }
                                            RdpOutputEvent::DesktopUpdate(update)=>{
                                                let (buffer,width,height,region)=update.into_parts();
                                                let rect=Rect {
                                                    x:region.left,y:region.top,
                                                    width:region.right.saturating_sub(region.left).saturating_add(1),
                                                    height:region.bottom.saturating_sub(region.top).saturating_add(1),
                                                };
                                                if compositor.apply(width.get(),height.get(),rect,&buffer).is_ok() {
                                                    if !outcome.first_image {
                                                        outcome.first_image=true;
                                                        notify(&app,&event_id,"first_image","Primeira atualização gráfica recebida",false);
                                                    }
                                                }else{
                                                    notify(&app,&event_id,"graphics_warning","Atualização gráfica rejeitada por dimensão inválida",false);
                                                }
                                            }
                                            RdpOutputEvent::Image{buffer,width,height}=>{
                                                // Fallback for server modes that send full images.
                                                let rect=Rect{x:0,y:0,width:width.get(),height:height.get()};
                                                let _=compositor.apply(width.get(),height.get(),rect,&buffer);
                                                if !outcome.first_image{
                                                    outcome.first_image=true;
                                                    notify(&app,&event_id,"first_image","Primeiro framebuffer recebido",false);
                                                }
                                            }
                                            _=>{}
                                        }
                                    }
                                    Some(ack) = ack_rx.recv() => {
                                        if inflight.is_some_and(|(expected,_)| expected==ack) {
                                            inflight=None;
                                        }
                                    }
                                    _ = ticker.tick() => {
                                        if inflight.is_some_and(|(_,instant)| instant.elapsed()>
                                            Duration::from_millis(ACK_TIMEOUT_MS)) {
                                            // A missing ACK must never lose desktop state.
                                            // Force a complete resynchronization before next delta.
                                            inflight=None;
                                            compositor.mark_full();
                                        }
                                        if inflight.is_none() && compositor.has_dirty() {
                                            let next_sequence=sequence.wrapping_add(1);
                                            if let Some(patch)=compositor.next_patch(next_sequence) {
                                                let rect=Rect{
                                                    x:patch.x,y:patch.y,width:patch.width,height:patch.height
                                                };
                                                if frame_tx.try_send(patch).is_ok(){
                                                    sequence=next_sequence;
                                                    inflight=Some((sequence,Instant::now()));
                                                }else{
                                                    // Re-merge the region with later updates, never discard deltas.
                                                    // Do not advance the sequence for an unsent patch.
                                                    compositor.restore_dirty(rect);
                                                }
                                            }
                                        }
                                    }
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
                // Disconnect the bounded queue BEFORE joining the encoder worker.
                // Without this drop, recv() waits forever and session teardown deadlocks.
                drop(frame_tx);
                if let Ok(worker) = worker { let _ = worker.join(); }
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

    pub fn ack_frame(&self,event_id:&str,sequence:u64)->Result<(),String>{
        let sessions=self.sessions.lock();
        let handle=sessions.values().find(|s|s.event_id==event_id)
            .ok_or("Sessão RDP não encontrada")?;
        handle.frame_ack.try_send(sequence)
            .map_err(|_|"Fila de confirmação gráfica ocupada ou encerrada".into())
    }

    pub fn send_input(&self,id: &str,input: InputAction) -> Result<(),String> {
        let event=to_fastpath(input)?;
        let guard=self.sessions.lock();
        let handle=guard.get(id).ok_or("Sessão RDP não está ativa")?;
        handle.sender.try_send(RdpInputEvent::FastPath(smallvec![event]))
            .map_err(|_| "Fila de eventos RDP ocupada ou encerrada".to_owned())
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
    fn default_policy_is_strict_and_override_is_explicit() {
        assert!(matches!(certificate_policy(false), CertificateValidation::Strict));
        assert!(matches!(
            certificate_policy(true),
            CertificateValidation::DangerouslyAcceptInvalidCertificate
        ));
    }
    #[test]
    fn keyboard_release_has_correct_flags() {
        let event=to_fastpath(InputAction::Key {scancode:0x1c,extended:false,pressed:false}).unwrap();
        assert!(matches!(event,FastPathInputEvent::KeyboardEvent(f,0x1c) if f.contains(KeyboardFlags::RELEASE)));
    }
    #[test]
    fn invalid_mouse_buttons_rejected() {
        assert!(to_fastpath(InputAction::Mouse {action:MouseAction::Down,x:1,y:1,button:9,wheel:0}).is_err());
    }
    #[test]
    fn status_classification_never_echoes_passwords() {
        assert_eq!(failure_label(&ConnectorErrorKind::AccessDenied),"Acesso negado pelo servidor RDP");
        assert_eq!(failure_label(&ConnectorErrorKind::General),"Falha RDP antes da confirmação do login (verifique certificado TLS, rede e credenciais)");
    }
}
