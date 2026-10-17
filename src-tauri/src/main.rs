#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod rdp;
mod rdp_engine;
mod rdp_frames;
mod rdp_compositor;
mod transfers;
use base64::{engine::general_purpose::STANDARD, Engine};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::{Read, Write}, net::{TcpStream, ToSocketAddrs}, sync::Arc, time::Duration};
use tauri::{Emitter, Manager};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile { id: String, name: String, protocol: String, host: String, port: u16, username: String, folder: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileInput { id: Option<String>, name: String, protocol: String, host: String, port: u16, username: String, folder: String }

struct SshSession { writer: Box<dyn Write + Send>, child: Box<dyn portable_pty::Child + Send>, master: Box<dyn portable_pty::MasterPty + Send> }
struct AppState { db: Mutex<Connection>, ssh: Arc<Mutex<HashMap<String, SshSession>>>, rdp: rdp_engine::Engine, files: transfers::Transfers }

#[tauri::command]
fn profiles_list(state: tauri::State<'_, AppState>) -> Result<Vec<Profile>, String> {
 let db=state.db.lock();
 let mut stmt=db.prepare("SELECT id,name,protocol,host,port,username,folder FROM profiles ORDER BY folder COLLATE NOCASE,name COLLATE NOCASE").map_err(|e|e.to_string())?;
 let result=stmt.query_map([],|r|Ok(Profile{id:r.get(0)?,name:r.get(1)?,protocol:r.get(2)?,host:r.get(3)?,port:r.get(4)?,username:r.get(5)?,folder:r.get(6)?})).map_err(|e|e.to_string())?;
 result.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
#[tauri::command]
fn profiles_save(state: tauri::State<'_, AppState>, input: ProfileInput) -> Result<String,String> {
 if input.name.trim().is_empty() || input.host.trim().is_empty() || !matches!(input.protocol.as_str(),"ssh"|"rdp"|"ftp"|"sftp"|"scp") || input.port==0 { return Err("Perfil inválido".into()); }
 if input.host.chars().any(|c| c.is_control()) || input.username.chars().any(|c| c.is_control()) { return Err("Caracteres de controle não permitidos".into()); }
 let folder=folder_path(&input.folder)?;
 let id=input.id.unwrap_or_else(||uuid::Uuid::new_v4().to_string());
 ensure_folders(&state.db.lock(),&folder)?;
 state.db.lock().execute("INSERT INTO profiles (id,name,protocol,host,port,username,folder) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET name=excluded.name,protocol=excluded.protocol,host=excluded.host,port=excluded.port,username=excluded.username,folder=excluded.folder", params![id,input.name,input.protocol,input.host,input.port,input.username,folder]).map_err(|e|e.to_string())?;
 Ok(id)
}
#[tauri::command]
fn profiles_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(),String> { state.db.lock().execute("DELETE FROM profiles WHERE id=?1",params![id]).map_err(|e|e.to_string())?; Ok(()) }

// Folder paths use slash-delimited segments; preserve legacy profile folder values.
fn folder_path(value:&str)->Result<String,String> {
 let cleaned=value.trim().trim_matches('/').to_string();
 if cleaned.is_empty() { return Ok("Geral".into()); }
 if cleaned.len()>500 || cleaned.split('/').any(|v|v.trim().is_empty()||v=="."||v=="..")
    || cleaned.chars().any(|c|c.is_control()||c=='\\') {
    return Err("Nome/caminho de pasta inválido".into());
 }
 Ok(cleaned)
}
#[tauri::command]
fn folders_list(state:tauri::State<'_,AppState>)->Result<Vec<String>,String>{
 let conn=state.db.lock();
 let mut stmt=conn.prepare("SELECT path FROM folders ORDER BY path COLLATE NOCASE").map_err(|e|e.to_string())?;
 let rows=stmt.query_map([],|r|r.get(0)).map_err(|e|e.to_string())?;
 rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
fn ensure_folders(db:&Connection,path:&str)->Result<(),String>{
 let mut current=String::new();
 for piece in path.split('/') {
  if !current.is_empty(){current.push('/');}
  current.push_str(piece);
  db.execute("INSERT OR IGNORE INTO folders(path) VALUES (?1)",params![current]).map_err(|e|e.to_string())?;
 }
 Ok(())
}
#[tauri::command]
fn folder_create(state:tauri::State<'_,AppState>,path:String)->Result<(),String>{
 let path=folder_path(&path)?;
 ensure_folders(&state.db.lock(),&path)
}
#[tauri::command]
fn folder_rename(state:tauri::State<'_,AppState>,old_path:String,new_path:String)->Result<(),String>{
 let old=folder_path(&old_path)?;
 let new=folder_path(&new_path)?;
 if old==new{return Ok(());}
 if new.starts_with(&(old.clone()+"/")){return Err("Não é permitido mover uma pasta para dentro dela mesma".into());}
 let mut db=state.db.lock();
 let transaction=db.transaction().map_err(|e|e.to_string())?;
 let mut names=Vec::new();
 {
  let mut stmt=transaction.prepare("SELECT path FROM folders WHERE path=?1 OR path LIKE ?2 ORDER BY LENGTH(path) ASC").map_err(|e|e.to_string())?;
  let rows=stmt.query_map(params![old,format!("{old}/%")],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;
  for p in rows {names.push(p.map_err(|e|e.to_string())?);}
 }
 if names.is_empty(){return Err("Pasta não encontrada".into());}
 let dest_exists=transaction.query_row("SELECT EXISTS(SELECT 1 FROM folders WHERE path=?1)",params![new],|r|r.get::<_,bool>(0)).map_err(|e|e.to_string())?;
 if dest_exists {return Err("A pasta de destino já existe".into());}
 // Parent folders may exist independently; add any missing parent.
 if let Some((parent,_))=new.rsplit_once('/'){ensure_folders(&transaction,parent)?;}
 // Insert all new paths first, then reassign profile paths and remove old ones.
 for old_sub in &names {
  let new_sub=format!("{}{}",&new,&old_sub[old.len()..]);
  transaction.execute("INSERT INTO folders(path) VALUES (?1)",params![new_sub]).map_err(|e|e.to_string())?;
 }
 transaction.execute("UPDATE profiles SET folder=?1 WHERE folder=?2",params![new,old]).map_err(|e|e.to_string())?;
 for sub in names.iter().skip(1){
  let replacement=format!("{}{}",&new,&sub[old.len()..]);
  transaction.execute("UPDATE profiles SET folder=?1 WHERE folder=?2",params![replacement,sub]).map_err(|e|e.to_string())?;
 }
 for path in names.iter().rev(){transaction.execute("DELETE FROM folders WHERE path=?1",params![path]).map_err(|e|e.to_string())?;}
 transaction.commit().map_err(|e|e.to_string())
}
#[tauri::command]
fn folder_delete(state:tauri::State<'_,AppState>,path:String)->Result<(),String>{
 let path=folder_path(&path)?;
 let db=state.db.lock();
 let descendants:i64=db.query_row("SELECT COUNT(*) FROM folders WHERE path LIKE ?1",params![format!("{path}/%")],|r|r.get(0)).map_err(|e|e.to_string())?;
 let profiles:i64=db.query_row("SELECT COUNT(*) FROM profiles WHERE folder=?1 OR folder LIKE ?2",params![path,format!("{path}/%")],|r|r.get(0)).map_err(|e|e.to_string())?;
 if descendants>0||profiles>0{return Err("Só é possível excluir pastas vazias, sem subpastas nem conexões".into());}
 db.execute("DELETE FROM folders WHERE path=?1",params![path]).map_err(|e|e.to_string())?;
 Ok(())
}
#[tauri::command]
fn profile_clone(state:tauri::State<'_,AppState>,id:String)->Result<String,String>{
 let db=state.db.lock();
 let profile:Profile=db.query_row(
  "SELECT id,name,protocol,host,port,username,folder FROM profiles WHERE id=?1",
  params![id],|r|Ok(Profile{id:r.get(0)?,name:r.get(1)?,protocol:r.get(2)?,host:r.get(3)?,port:r.get(4)?,username:r.get(5)?,folder:r.get(6)?})
 ).map_err(|e|e.to_string())?;
 let new_id=uuid::Uuid::new_v4().to_string();
 db.execute("INSERT INTO profiles(id,name,protocol,host,port,username,folder) VALUES(?1,?2,?3,?4,?5,?6,?7)",
 params![new_id,format!("{} (cópia)",profile.name),profile.protocol,profile.host,profile.port,profile.username,profile.folder])
 .map_err(|e|e.to_string())?;
 Ok(new_id)
}
fn transfer_profile(state:&tauri::State<'_,AppState>,id:&str)->Result<(String,String,u16,String),String>{
 state.db.lock().query_row("SELECT protocol,host,port,username FROM profiles WHERE id=?1",
  params![id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())
}
#[tauri::command]
fn transfer_open(state:tauri::State<'_,AppState>,profile_id:String,password:String,allow_ftp:bool)->Result<String,String>{
 let (protocol,host,port,user)=transfer_profile(&state,&profile_id)?;
 state.files.connect(&protocol,&host,port,&user,&password,allow_ftp)
}
#[tauri::command]
fn transfer_close(state:tauri::State<'_,AppState>,id:String){state.files.close(&id);}
#[tauri::command]
fn transfer_list(state:tauri::State<'_,AppState>,id:String,path:String)->Result<Vec<transfers::Entry>,String>{
 state.files.list(&id,&path)
}
#[tauri::command]
fn transfer_mkdir(state:tauri::State<'_,AppState>,id:String,path:String)->Result<(),String>{
 state.files.mkdir(&id,&path)
}
#[tauri::command]
fn transfer_delete(state:tauri::State<'_,AppState>,id:String,path:String,directory:bool)->Result<(),String>{
 state.files.remove(&id,&path,directory)
}
#[tauri::command]
fn transfer_copy(app:tauri::AppHandle,state:tauri::State<'_,AppState>,id:String,remote:String,local:String,upload:bool,overwrite:bool)->Result<u64,String>{
 state.files.transfer(&app,&id,&remote,std::path::Path::new(&local),upload,overwrite)
}
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
// Phase Zero RDP milestone: TCP preflight only. No credentials, no protocol login.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RdpPreflight { address: String, tcp_reachable: bool, note: String }
#[tauri::command]
fn rdp_preflight(state: tauri::State<'_, AppState>, profile_id: String) -> Result<RdpPreflight,String> {
 let (protocol,host,port):(String,String,u16)=state.db.lock()
   .query_row("SELECT protocol,host,port FROM profiles WHERE id=?1",params![profile_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
   .map_err(|e|e.to_string())?;
 if protocol != "rdp" { return Err("Este perfil não utiliza RDP".into()); }
 if host.trim().is_empty() || host.chars().any(|c| c.is_control() || c.is_whitespace()) || port == 0 {
   return Err("Endereço ou porta inválida".into());
 }
 let addr=if host.contains(':') && !host.starts_with('['){format!("[{host}]:{port}")}else{format!("{host}:{port}")};
 let targets=addr.to_socket_addrs().map_err(|e|format!("Falha ao resolver host: {e}"))?;
 let mut reachable=false;
 for target in targets.take(4) {if TcpStream::connect_timeout(&target,Duration::from_secs(2)).is_ok(){reachable=true;break;}}
 Ok(RdpPreflight{address:addr,tcp_reachable:reachable,note:if reachable{
   "TCP acessível. NLA, autenticação e desktop remoto ainda não implementados.".into()
 }else{"TCP indisponível. Verifique firewall, destino e RDP.".into()}})
}
#[tauri::command]
fn rdp_negotiate(state: tauri::State<'_, AppState>, profile_id: String) -> Result<rdp::NegotiationResult,String> {
    let (protocol,host,port):(String,String,u16)=state.db.lock()
      .query_row("SELECT protocol,host,port FROM profiles WHERE id=?1",params![profile_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
      .map_err(|e|e.to_string())?;
    if protocol != "rdp" {return Err("Perfil não é RDP".into());}
    rdp::probe(&host,port)
}
#[tauri::command]
fn rdp_start(app: tauri::AppHandle, state: tauri::State<'_, AppState>, profile_id: String, password: String, event_id: String, allow_invalid_certificate: bool) -> Result<String,String> {
    let (protocol,host,port,username):(String,String,u16,String)=state.db.lock()
       .query_row("SELECT protocol,host,port,username FROM profiles WHERE id=?1",params![profile_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
       .map_err(|e|e.to_string())?;
    if protocol != "rdp" {return Err("Perfil não é RDP".into());}
    state.rdp.start(app,host,port,username,password,event_id,allow_invalid_certificate)
}
#[tauri::command]
fn rdp_close(state: tauri::State<'_, AppState>, id:String) {
    state.rdp.close(&id);
}
#[tauri::command]
fn rdp_input(state: tauri::State<'_, AppState>, id:String, input:rdp_engine::InputAction) -> Result<(),String> {
    state.rdp.send_input(&id,input)
}
#[tauri::command]
fn rdp_frame_ack(state:tauri::State<'_,AppState>,event_id:String,sequence:u64)->Result<(),String>{
    state.rdp.ack_frame(&event_id,sequence)
}
fn main(){
 tauri::Builder::default().setup(|app|{
    let mut dir=app.path().app_data_dir()?;std::fs::create_dir_all(&dir)?;dir.push("profiles.sqlite3");
    let conn=Connection::open(dir)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    // Existing 0.0.5 databases restrict profiles to ssh/rdp with a CHECK constraint.
    // SQLite cannot alter CHECK in-place: copy rows transactionally and preserve IDs.
    let old_schema:String=conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='profiles'",[],|r|r.get(0)
    ).unwrap_or_default();
    if !old_schema.is_empty() && !old_schema.to_lowercase().contains("'sftp'") {
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE profiles_new (id TEXT PRIMARY KEY,name TEXT NOT NULL,
                protocol TEXT NOT NULL CHECK(protocol IN ('ssh','rdp','ftp','sftp','scp')),
                host TEXT NOT NULL,port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535),
                username TEXT NOT NULL DEFAULT '',folder TEXT NOT NULL DEFAULT 'Geral');
            INSERT INTO profiles_new(id,name,protocol,host,port,username,folder)
               SELECT id,name,protocol,host,port,username,folder FROM profiles;
            DROP TABLE profiles;
            ALTER TABLE profiles_new RENAME TO profiles;
            COMMIT;")?;
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS profiles(
       id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL
         CHECK(protocol IN ('ssh','rdp','ftp','sftp','scp')),
       host TEXT NOT NULL,port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535),
       username TEXT NOT NULL DEFAULT '',folder TEXT NOT NULL DEFAULT 'Geral');
       CREATE TABLE IF NOT EXISTS folders(path TEXT NOT NULL PRIMARY KEY);")?;
    {let mut stmt=conn.prepare("SELECT DISTINCT folder FROM profiles")?;
      let existing=stmt.query_map([],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
      for path in existing {if let Ok(path)=folder_path(&path) {ensure_folders(&conn,&path).map_err(std::io::Error::other)?;}}
    }
    app.manage(AppState{db:Mutex::new(conn),ssh:Arc::new(Mutex::new(HashMap::new())),rdp:rdp_engine::Engine::new(),files:transfers::Transfers::new()});Ok(())
 }).invoke_handler(tauri::generate_handler![profiles_list,profiles_save,profiles_delete,profile_clone,folders_list,folder_create,folder_rename,folder_delete,transfer_open,transfer_close,transfer_list,transfer_mkdir,transfer_delete,transfer_copy,transfers::pick_local_file,transfers::pick_save_file,ssh_open,ssh_write,ssh_resize,ssh_close,rdp_preflight,rdp_negotiate,rdp_start,rdp_close,rdp_input,rdp_frame_ack]).run(tauri::generate_context!()).expect("Erro ao executar RemoteHub");
}
