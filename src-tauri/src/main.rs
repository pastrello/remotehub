#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use base64::{engine::general_purpose::STANDARD, Engine};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::{Read, Write}, sync::Arc};
use tauri::{Emitter, Manager};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile { id: String, name: String, protocol: String, host: String, port: u16, username: String, folder: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileInput { id: Option<String>, name: String, protocol: String, host: String, port: u16, username: String, folder: String }

struct SshSession { writer: Box<dyn Write + Send>, child: Box<dyn portable_pty::Child + Send>, master: Box<dyn portable_pty::MasterPty + Send> }
struct AppState { db: Mutex<Connection>, ssh: Arc<Mutex<HashMap<String, SshSession>>> }

#[tauri::command]
fn profiles_list(state: tauri::State<'_, AppState>) -> Result<Vec<Profile>, String> {
 let db=state.db.lock();
 let mut stmt=db.prepare("SELECT id,name,protocol,host,port,username,folder FROM profiles ORDER BY folder COLLATE NOCASE,name COLLATE NOCASE").map_err(|e|e.to_string())?;
 let result=stmt.query_map([],|r|Ok(Profile{id:r.get(0)?,name:r.get(1)?,protocol:r.get(2)?,host:r.get(3)?,port:r.get(4)?,username:r.get(5)?,folder:r.get(6)?})).map_err(|e|e.to_string())?;
 result.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
#[tauri::command]
fn profiles_save(state: tauri::State<'_, AppState>, input: ProfileInput) -> Result<String,String> {
 if input.name.trim().is_empty() || input.host.trim().is_empty() || !matches!(input.protocol.as_str(),"ssh"|"rdp") || input.port==0 { return Err("Perfil inválido".into()); }
 if input.host.chars().any(|c| c.is_control()) || input.username.chars().any(|c| c.is_control()) { return Err("Caracteres de controle não permitidos".into()); }
 let id=input.id.unwrap_or_else(||uuid::Uuid::new_v4().to_string());
 state.db.lock().execute("INSERT INTO profiles (id,name,protocol,host,port,username,folder) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET name=excluded.name,protocol=excluded.protocol,host=excluded.host,port=excluded.port,username=excluded.username,folder=excluded.folder", params![id,input.name,input.protocol,input.host,input.port,input.username,input.folder]).map_err(|e|e.to_string())?;
 Ok(id)
}
#[tauri::command]
fn profiles_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(),String> { state.db.lock().execute("DELETE FROM profiles WHERE id=?1",params![id]).map_err(|e|e.to_string())?; Ok(()) }

// Temporary phase-zero adapter: OpenSSH ssh.exe via PTY, no persisted secrets.
#[tauri::command]
fn ssh_open(app: tauri::AppHandle, state: tauri::State<'_,AppState>, profile_id: String) -> Result<String,String> {
 let (protocol,host,port,username): (String,String,u16,String) = state.db.lock().query_row("SELECT protocol,host,port,username FROM profiles WHERE id=?1",params![profile_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
 if protocol!="ssh" { return Err("Somente perfis SSH podem iniciar terminais".into()); }
 if host.starts_with('-') || username.starts_with('-') || host.trim().is_empty() || host.chars().any(|c|c.is_whitespace()) || username.chars().any(|c|c.is_whitespace()) {return Err("Host/usuário inválidos para SSH".into());}
 let pair=native_pty_system().openpty(PtySize{rows:30,cols:100,pixel_width:0,pixel_height:0}).map_err(|e|e.to_string())?;
 let mut cmd=CommandBuilder::new("ssh");
 cmd.arg("-p"); cmd.arg(port.to_string());
 cmd.arg("-o"); cmd.arg("StrictHostKeyChecking=ask");
 cmd.arg(if username.is_empty(){host.clone()}else{format!("{}@{}",username,host)});
 let child=pair.slave.spawn_command(cmd).map_err(|e|format!("Não foi possível iniciar OpenSSH: {e}"))?;
 drop(pair.slave);
 let mut reader=pair.master.try_clone_reader().map_err(|e|e.to_string())?;
 let writer=pair.master.take_writer().map_err(|e|e.to_string())?;
 let sid=uuid::Uuid::new_v4().to_string();
 let sid_read=sid.clone();
 let sessions=state.ssh.clone();
 sessions.lock().insert(sid.clone(),SshSession{writer,child,master:pair.master});
 std::thread::spawn(move||{
   let mut buf=[0u8;8192];
   loop {match reader.read(&mut buf){Ok(0)|Err(_)=>break,Ok(n)=>{let _=app.emit("ssh-output",serde_json::json!({"id":sid_read,"data":STANDARD.encode(&buf[..n])}));}}}
   let _=app.emit("ssh-closed",serde_json::json!({"id":sid_read}));
   sessions.lock().remove(&sid_read);
 });
 Ok(sid)
}
#[tauri::command]
fn ssh_write(state:tauri::State<'_,AppState>,id:String,data:String)->Result<(),String>{
 let decoded=STANDARD.decode(data).map_err(|e|e.to_string())?;
 let mut guard=state.ssh.lock();let session=guard.get_mut(&id).ok_or("Sessão encerrada")?;
 session.writer.write_all(&decoded).map_err(|e|e.to_string())
}
#[tauri::command]
fn ssh_resize(state:tauri::State<'_,AppState>,id:String,rows:u16,cols:u16)->Result<(),String>{
 let mut guard=state.ssh.lock();let session=guard.get_mut(&id).ok_or("Sessão encerrada")?;
 session.master.resize(PtySize{rows:rows.max(1),cols:cols.max(1),pixel_width:0,pixel_height:0}).map_err(|e|e.to_string())
}
#[tauri::command]
fn ssh_close(state:tauri::State<'_,AppState>,id:String)->Result<(),String>{
 if let Some(mut session)=state.ssh.lock().remove(&id){session.child.kill().map_err(|e|e.to_string())?;}
 Ok(())
}
fn main(){
 tauri::Builder::default().setup(|app|{
    let mut dir=app.path().app_data_dir()?;std::fs::create_dir_all(&dir)?;dir.push("profiles.sqlite3");
    let conn=Connection::open(dir)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;CREATE TABLE IF NOT EXISTS profiles (id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL CHECK(protocol IN ('ssh','rdp')),host TEXT NOT NULL,port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535),username TEXT NOT NULL DEFAULT '',folder TEXT NOT NULL DEFAULT 'Geral');")?;
    app.manage(AppState{db:Mutex::new(conn),ssh:Arc::new(Mutex::new(HashMap::new()))});Ok(())
 }).invoke_handler(tauri::generate_handler![profiles_list,profiles_save,profiles_delete,ssh_open,ssh_write,ssh_resize,ssh_close]).run(tauri::generate_context!()).expect("Erro ao executar RemoteHub");
}
