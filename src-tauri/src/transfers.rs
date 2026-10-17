//! File transfer sessions: SFTP and SCP over verified SSH; legacy FTP opt-in.
//! Credentials live only in memory and are never stored in the connection DB.
use std::{collections::HashMap,fs::{self,File},io::{self,Read,Write},net::{TcpStream,ToSocketAddrs},path::{Path,PathBuf},str::FromStr,sync::Arc,time::{Duration,Instant}};
use parking_lot::Mutex;
use serde::Serialize;
use ssh2::{CheckResult,KnownHostFileKind,OpenFlags,OpenType,Session,Sftp};
use suppaftp::{FtpStream,types::FileType as FtpType};
use tauri::{AppHandle,Emitter};

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct Entry {pub name:String,pub path:String,pub directory:bool,pub size:u64}
#[derive(Serialize,Clone)]
#[serde(rename_all="camelCase")]
pub struct TransferEvent {pub id:String,pub operation:String,pub transferred:u64,pub total:u64,pub done:bool,pub error:Option<String>}
enum Connection {
    Sftp {ssh:Session,sftp:Sftp},
    Scp {ssh:Session,sftp:Option<Sftp>},
    Ftp {ftp:FtpStream},
}
pub struct Transfers {sessions:Mutex<HashMap<String,Arc<Mutex<Connection>>>>}
impl Transfers {
    pub fn new()->Self {Self {sessions:Mutex::new(HashMap::new())}}
    pub fn connect(&self,protocol:&str,host:&str,port:u16,user:&str,password:&str,allow_ftp:bool)
        -> Result<String,String>
    {
        if host.is_empty()||port==0||host.chars().any(|c|c.is_whitespace()||c.is_control()||c=='@')
            ||user.chars().any(|c|c.is_control()) {return Err("Host ou usuário inválidos".into());}
        let connection=if protocol=="ftp" {
            if !allow_ftp {return Err("FTP transmite senhas e arquivos sem criptografia; confirme explicitamente o risco".into());}
            let mut ftp=FtpStream::connect((host,port)).map_err(|e|format!("Falha de rede FTP: {e}"))?;
            ftp.get_ref().set_read_timeout(Some(Duration::from_secs(20))).map_err(|e|e.to_string())?;
            ftp.get_ref().set_write_timeout(Some(Duration::from_secs(20))).map_err(|e|e.to_string())?;
            ftp.login(user,password).map_err(|_|"Login FTP recusado".to_string())?;
            ftp.transfer_type(FtpType::Binary).map_err(|e|e.to_string())?;
            Connection::Ftp{ftp}
        }else if matches!(protocol,"ssh"|"sftp"|"scp"){
            let address=if host.contains(':')&&!host.starts_with('['){format!("[{host}]:{port}")}else{format!("{host}:{port}")};
            let addresses=address.to_socket_addrs().map_err(|e|format!("DNS/Host: {e}"))?;
            let mut connected=None;
            for addr in addresses.take(6){
                if let Ok(sock)=TcpStream::connect_timeout(&addr,Duration::from_secs(8)){connected=Some(sock);break;}
            }
            let socket=connected.ok_or("Não foi possível alcançar o SSH na porta indicada")?;
            socket.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e|e.to_string())?;
            socket.set_write_timeout(Some(Duration::from_secs(30))).map_err(|e|e.to_string())?;
            let mut ssh=Session::new().map_err(|e|e.to_string())?;
            ssh.set_tcp_stream(socket);
            ssh.set_timeout(30_000);
            ssh.handshake().map_err(|e|format!("Handshake SSH: {e}"))?;
            verify_host(&ssh,host,port)?;
            if password.is_empty(){
                if ssh.userauth_agent(user).is_err(){
                    let home=home_dir()?;
                    let mut ok=false;
                    for name in ["id_ed25519","id_ecdsa","id_rsa"] {
                        let path=home.join(".ssh").join(name);
                        if path.is_file() && ssh.userauth_pubkey_file(user,None,&path,None).is_ok(){ok=true;break;}
                    }
                    if !ok{return Err("Autenticação SSH falhou: inicie o ssh-agent ou informe uma senha".into());}
                }
            }else{
                ssh.userauth_password(user,password)
                    .map_err(|_|"Autenticação SSH com senha recusada".to_string())?;
            }
            if !ssh.authenticated(){return Err("SSH não autenticado".into());}
            if protocol=="scp" {
                let sftp=ssh.sftp().ok(); // SCP works even when SFTP subsystem is disabled.
                Connection::Scp {ssh,sftp}
            }else{
                let sftp=ssh.sftp().map_err(|e|format!("Servidor não oferece subsistema SFTP: {e}"))?;
                Connection::Sftp {ssh,sftp}
            }
        }else {return Err("Protocolo de transferência desconhecido".into())};
        let id=uuid::Uuid::new_v4().to_string();
        self.sessions.lock().insert(id.clone(),Arc::new(Mutex::new(connection)));
        Ok(id)
    }
    fn session(&self,id:&str)->Result<Arc<Mutex<Connection>>,String>{
        self.sessions.lock().get(id).cloned().ok_or("Sessão de arquivos encerrada".into())
    }
    pub fn close(&self,id:&str) {self.sessions.lock().remove(id);}
    pub fn list(&self,id:&str,path:&str)->Result<Vec<Entry>,String>{
        let session=self.session(id)?;
        let mut guard=session.lock();
        let path=remote_path(path)?;
        let mut entries=match &mut *guard {
            Connection::Sftp{sftp,..}=>sftp_list(sftp,&path)?,
            Connection::Scp{sftp:Some(sftp),..}=>sftp_list(sftp,&path)?,
            Connection::Scp{sftp:None,..}=>return Err("Servidor SCP não oferece listagem SFTP. Use caminhos remotos completos para enviar/receber.".into()),
            Connection::Ftp{ftp}=>{
                ftp.cwd(path.as_str()).map_err(|e|format!("Diretório FTP: {e}"))?;
                let lines=ftp.list(None).map_err(|e|format!("Listagem FTP: {e}"))?;
                let mut result=Vec::new();
                for line in lines {
                    let file=suppaftp::list::File::from_str(&line);
                    if let Ok(f)=file {
                        let name=f.name().to_owned();
                        if name=="."||name==".." {continue;}
                        result.push(Entry {path:join_remote(&path,&name),name,directory:f.is_directory(),size:f.size() as u64});
                    }
                }
                result
            }
        };
        entries.sort_by(|a,b|b.directory.cmp(&a.directory).then_with(||a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    }
    pub fn mkdir(&self,id:&str,path:&str)->Result<(),String> {
        let path=remote_path(path)?;
        let session=self.session(id)?;let mut guard=session.lock();
        match &mut *guard {
            Connection::Sftp{sftp,..}|Connection::Scp{sftp:Some(sftp),..}=>
                sftp.mkdir(Path::new(&path),0o755).map_err(|e|e.to_string()),
            Connection::Ftp{ftp}=>ftp.mkdir(path).map_err(|e|e.to_string()),
            Connection::Scp{sftp:None,..}=>Err("Criar pasta exige SFTP neste servidor".into())
        }
    }
    pub fn remove(&self,id:&str,path:&str,directory:bool)->Result<(),String>{
        let path=remote_path(path)?;
        if path=="/" {return Err("Não é possível remover o diretório raiz".into());}
        let session=self.session(id)?;let mut guard=session.lock();
        match &mut *guard {
            Connection::Sftp{sftp,..}|Connection::Scp{sftp:Some(sftp),..}=>{
                if directory{sftp.rmdir(Path::new(&path)).map_err(|e|e.to_string())}
                else{sftp.unlink(Path::new(&path)).map_err(|e|e.to_string())}
            },
            Connection::Ftp{ftp}=>{
                if directory{ftp.rmdir(path).map_err(|e|e.to_string())}
                else{ftp.rm(path).map_err(|e|e.to_string())}
            },
            Connection::Scp{sftp:None,..}=>Err("Exclusão exige SFTP neste servidor".into())
        }
    }
    pub fn transfer(&self,app:&AppHandle,id:&str,remote:&str,local:&Path,upload:bool,overwrite:bool)
        -> Result<u64,String>
    {
        let remote=remote_path(remote)?;
        if remote=="/" {return Err("Escolha um arquivo, não o diretório raiz".into());}
        let session=self.session(id)?;
        let mut guard=session.lock();
        let op=if upload {"upload"}else{"download"};
        let emit=|n:u64,total:u64,done:bool,error:Option<String>|{
            let _=app.emit("transfer-progress",TransferEvent{id:id.to_owned(),operation:op.into(),transferred:n,total,done,error});
        };
        if upload {
            let mut file=File::open(local).map_err(|e|format!("Arquivo local: {e}"))?;
            let size=file.metadata().map_err(|e|e.to_string())?.len();
            let mut wrapped=ProgressReader::new(&mut file,size,|n|emit(n,size,false,None));
            match &mut *guard {
                Connection::Sftp{sftp,..}=>{
                    let mut flags=OpenFlags::WRITE|OpenFlags::CREATE;
                    if overwrite{flags|=OpenFlags::TRUNCATE;}else{flags|=OpenFlags::EXCLUSIVE;}
                    let mut dest=sftp.open_mode(Path::new(&remote),flags,0o644,OpenType::File)
                        .map_err(|e|format!("Abrir arquivo remoto: {e}"))?;
                    io::copy(&mut wrapped,&mut dest).map_err(|e|format!("Upload SFTP: {e}"))?;
                    dest.flush().map_err(|e|e.to_string())?;
                }
                Connection::Scp{ssh,sftp:_}=>{
                    // SCP cannot guarantee no-clobber, even when a preliminary
                    // SFTP stat succeeds (TOCTOU race). Require user confirmation.
                    if !overwrite {
                        return Err("SCP exige confirmação de sobrescrita; não oferece criação exclusiva atômica".into());
                    }
                    let mut dest=ssh.scp_send(Path::new(&remote),0o644,size,None).map_err(|e|format!("SCP send: {e}"))?;
                    io::copy(&mut wrapped,&mut dest).map_err(|e|format!("SCP upload: {e}"))?;
                    dest.send_eof().map_err(|e|e.to_string())?;
                    dest.wait_eof().map_err(|e|e.to_string())?;
                    dest.close().map_err(|e|e.to_string())?;
                    dest.wait_close().map_err(|e|e.to_string())?;
                }
                Connection::Ftp{ftp}=>{
                    if !overwrite {
                        // SIZE is optional in FTP, so an error cannot prove absence.
                        // Use NLST and fail closed when the server cannot list.
                        let (parent,name)=remote.rsplit_once('/').ok_or("Caminho FTP inválido")?;
                        ftp.cwd(if parent.is_empty(){"/"}else{parent}).map_err(|e|format!("Diretório remoto: {e}"))?;
                        let names=ftp.nlst(None).map_err(|_|"Servidor FTP não permite verificar existência. Confirme sobrescrita para prosseguir.".to_owned())?;
                        if names.iter().any(|p|p.trim_end_matches('/').rsplit('/').next()==Some(name)){
                            return Err("Arquivo FTP já existe. Confirme sobrescrita.".into());
                        }
                    }
                    ftp.put_file(remote,&mut wrapped).map_err(|e|format!("Upload FTP: {e}"))?;
                }
            }
            emit(size,size,true,None);
            Ok(size)
        }else{
            let parent=local.parent().ok_or("Destino local inválido")?;
            if !parent.is_dir(){return Err("Pasta local de destino não existe".into());}
            if local.exists()&&!overwrite{return Err("Arquivo local já existe; habilite sobrescrita".into());}
            let tmp=parent.join(format!(".remotehub-{}.part",uuid::Uuid::new_v4()));
            let result=(||->Result<u64,String>{
                let mut dest=File::create_new(&tmp).map_err(|e|format!("Criar arquivo temporário: {e}"))?;
                let (size,total)=match &mut *guard {
                    Connection::Sftp{sftp,..}=>{
                        let mut src=sftp.open(Path::new(&remote)).map_err(|e|format!("Download SFTP: {e}"))?;
                        let total=src.stat().ok().and_then(|s|s.size).unwrap_or(0);
                        let mut wrapped=ProgressWriter::new(&mut dest,total,|n|emit(n,total,false,None));
                        let copied=io::copy(&mut src,&mut wrapped).map_err(|e|format!("Download SFTP: {e}"))?;
                        (copied,total)
                    }
                    Connection::Scp{ssh,..}=>{
                        let (mut src,stat)=ssh.scp_recv(Path::new(&remote)).map_err(|e|format!("SCP receive: {e}"))?;
                        let total=stat.size();
                        let mut wrapped=ProgressWriter::new(&mut dest,total,|n|emit(n,total,false,None));
                        let copied=io::copy(&mut src,&mut wrapped).map_err(|e|format!("SCP download: {e}"))?;
                        src.send_eof().map_err(|e|e.to_string())?;
                        src.wait_eof().map_err(|e|e.to_string())?;
                        src.close().map_err(|e|e.to_string())?;
                        src.wait_close().map_err(|e|e.to_string())?;
                        (copied,total)
                    }
                    Connection::Ftp{ftp}=>{
                        let mut stream=ftp.retr_as_stream(&remote).map_err(|e|format!("Download FTP: {e}"))?;
                        let mut wrapped=ProgressWriter::new(&mut dest,0,|n|emit(n,0,false,None));
                        let copied=io::copy(&mut stream,&mut wrapped).map_err(|e|format!("Download FTP: {e}"))?;
                        stream.finish().map_err(|e|format!("Finalização FTP: {e}"))?;
                        (copied,0)
                    }
                };
                if total>0&&size!=total{return Err("Download incompleto: tamanho divergente".into());}
                dest.sync_all().map_err(|e|e.to_string())?;
                drop(dest);
                if local.exists(){
                    if !overwrite{return Err("Arquivo local já existe".into());}
                    // Preserve the old destination until the new file has been
                    // safely moved. A failed replacement must not destroy it.
                    let backup=parent.join(format!(".remotehub-{}.backup",uuid::Uuid::new_v4()));
                    fs::rename(local,&backup).map_err(|e|format!("Preparar backup local: {e}"))?;
                    if let Err(e)=fs::rename(&tmp,local){
                        let _=fs::rename(&backup,local);
                        return Err(format!("Falha substituindo arquivo local (original preservado): {e}"));
                    }
                    let _=fs::remove_file(&backup);
                }else{
                    fs::rename(&tmp,local).map_err(|e|format!("Mover arquivo baixado: {e}"))?;
                }
                Ok(size)
            })();
            if result.is_err(){let _=fs::remove_file(&tmp);}
            match result{Ok(size)=>{emit(size,size,true,None);Ok(size)},Err(e)=>{emit(0,0,true,Some(e.clone()));Err(e)}}
        }
    }
}
fn sftp_list(sftp:&Sftp,path:&str)->Result<Vec<Entry>,String>{
    let list=sftp.readdir(Path::new(path)).map_err(|e|format!("Listagem SFTP: {e}"))?;
    Ok(list.into_iter().filter_map(|(p,stat)|{
        let name=p.file_name()?.to_string_lossy().into_owned();
        if name=="."||name==".."{return None;}
        Some(Entry{
            path:join_remote(path,&name),
            name,directory:stat.is_dir(),size:stat.size.unwrap_or(0)
        })
    }).collect())
}
fn remote_path(path:&str)->Result<String,String>{
    if path.len()>4096||path.contains('\\')||path.chars().any(|c|c.is_control()){
        return Err("Caminho remoto inválido".into());
    }
    let p=if path.trim().is_empty(){"/"}else{path};
    if !p.starts_with('/') {return Err("Use caminhos remotos absolutos iniciados em /".into());}
    if p.split('/').any(|seg|seg==".."||seg==".") {return Err("Caminho remoto não pode conter . ou ..".into());}
    Ok(p.to_owned())
}
fn join_remote(parent:&str,name:&str)->String{
    format!("{}/{}",parent.trim_end_matches('/'),name)
}
fn home_dir()->Result<PathBuf,String>{
    std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME"))
        .map(PathBuf::from).ok_or("Não foi possível localizar a pasta do usuário".into())
}
fn verify_host(session:&Session,host:&str,port:u16)->Result<(),String>{
    let file=home_dir()?.join(".ssh").join("known_hosts");
    let (key,_)=session.host_key().ok_or("Servidor não forneceu chave SSH")?;
    let mut hosts=session.known_hosts().map_err(|e|e.to_string())?;
    hosts.read_file(&file,KnownHostFileKind::OpenSSH)
        .map_err(|_|"Arquivo known_hosts ausente ou inválido. Conecte uma vez pelo terminal SSH para confirmar a chave do servidor.".to_owned())?;
    match hosts.check_port(host,port,key){
        CheckResult::Match=>Ok(()),
        CheckResult::Mismatch=>Err("ALERTA: a chave SSH do servidor mudou! Conexão bloqueada.".into()),
        CheckResult::NotFound=>Err("Chave SSH desconhecida. Use primeiro o terminal SSH do RemoteHub para confirmar o servidor.".into()),
        CheckResult::Failure=>Err("Não foi possível validar a chave SSH do servidor.".into())
    }
}
struct ProgressReader<'a,R,F>{inner:&'a mut R,total:u64,count:u64,callback:F,last_update:Instant}
impl<'a,R,F> ProgressReader<'a,R,F>{fn new(inner:&'a mut R,total:u64,callback:F)->Self{Self{inner,total,count:0,callback,last_update:Instant::now()-Duration::from_secs(1)}}}
impl<R:Read,F:FnMut(u64)> Read for ProgressReader<'_,R,F>{
    fn read(&mut self,buf:&mut [u8])->io::Result<usize>{
        let n=self.inner.read(buf)?;self.count+=n as u64;
        if self.last_update.elapsed()>=Duration::from_millis(160){
            (self.callback)(self.count.min(self.total));
            self.last_update=Instant::now();
        }
        Ok(n)
    }
}
struct ProgressWriter<'a,W,F>{inner:&'a mut W,total:u64,count:u64,callback:F,last_update:Instant}
impl<'a,W,F> ProgressWriter<'a,W,F>{fn new(inner:&'a mut W,total:u64,callback:F)->Self{Self{inner,total,count:0,callback,last_update:Instant::now()-Duration::from_secs(1)}}}
impl<W:Write,F:FnMut(u64)> Write for ProgressWriter<'_,W,F>{
    fn write(&mut self,buf:&[u8])->io::Result<usize>{
        let n=self.inner.write(buf)?;self.count+=n as u64;
        if self.last_update.elapsed()>=Duration::from_millis(160){
            (self.callback)(if self.total>0{self.count.min(self.total)}else{self.count});
            self.last_update=Instant::now();
        }
        Ok(n)
    }
    fn flush(&mut self)->io::Result<()> {self.inner.flush()}
}
#[tauri::command]
pub fn pick_local_file()->Option<String>{
    rfd::FileDialog::new().pick_file().map(|p|p.to_string_lossy().into_owned())
}
#[tauri::command]
pub fn pick_save_file(suggested_name:String)->Option<String>{
    let safe=Path::new(&suggested_name).file_name().unwrap_or_default().to_string_lossy().into_owned();
    rfd::FileDialog::new().set_file_name(&safe).save_file().map(|p|p.to_string_lossy().into_owned())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn forbid_path_traversal_and_control(){assert!(remote_path("/root/../etc").is_err());assert!(remote_path("/foo\r\n").is_err());}
    #[test] fn canonical_remote_join(){assert_eq!(join_remote("/tmp","a.txt"),"/tmp/a.txt");}
}
