import React,{useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import type {Profile} from "./connection_sidebar";
type Entry={name:string;path:string;directory:boolean;size:number};
type TransferEvent={id:string;operation:string;transferred:number;total:number;done:boolean;error?:string|null};
function parent(path:string){const parts=path.split("/").filter(Boolean);parts.pop();return "/"+parts.join("/");}
function size(bytes:number){if(!bytes)return "0 B";if(bytes<1024)return bytes+" B";if(bytes<1048576)return (bytes/1024).toFixed(1)+" KiB";return (bytes/1048576).toFixed(1)+" MiB";}
export function TransferPanel({profile}:{profile:Profile}){
 const [password,setPassword]=useState("");
 const [allowFtp,setAllowFtp]=useState(false);
 const [session,setSession]=useState("");
 const sessionRef=useRef("");
 const [path,setPath]=useState("/");
 const [pathEntry,setPathEntry]=useState("/");
 const [remoteFile,setRemoteFile]=useState("");
 const [entries,setEntries]=useState<Entry[]>([]);
 const [selected,setSelected]=useState<Entry|null>(null);
 const [connected,setConnected]=useState(false);
 const [busy,setBusy]=useState(false);
 const [status,setStatus]=useState("Conecte para navegar ou transferir arquivos.");
 const [progress,setProgress]=useState<TransferEvent|null>(null);
 const [overwrite,setOverwrite]=useState(false);
 const alive=useRef(true);
 useEffect(()=>{
  alive.current=true;
  let off:(()=>void)|undefined;
  void listen<TransferEvent>("transfer-progress",e=>{
   if(alive.current&&e.payload.id===sessionRef.current)setProgress(e.payload);
  }).then(f=>{if(alive.current)off=f;else f()});
  return()=>{alive.current=false;off?.();if(sessionRef.current)void invoke("transfer_close",{id:sessionRef.current}).catch(()=>{});};
 },[]);
 async function withBusy(action:()=>Promise<void>){
  if(busy)return;setBusy(true);setProgress(null);
  try{await action()}catch(e){setStatus("Erro: "+String(e))}
  finally{if(alive.current)setBusy(false);}
 }
 async function refresh(id=sessionRef.current,folder=path){
  if(!id)return;
  try{
   const rows=await invoke<Entry[]>("transfer_list",{id,path:folder});
   if(!alive.current)return;
   setEntries(rows);setPath(folder);setPathEntry(folder);setSelected(null);
   setStatus(rows.length+" itens em "+folder);
  }catch(e){setEntries([]);setPath(folder);setPathEntry(folder);setStatus("Listagem: "+String(e)+". Para SCP sem SFTP, informe caminho remoto completo.");}
 }
 async function connect(){
  if(busy||session)return;
  if(profile.protocol==="ftp"&&!allowFtp){setStatus("FTP sem criptografia: marque a confirmação para conectar.");return;}
  await withBusy(async()=>{
   setStatus("Conectando...");
   const id=await invoke<string>("transfer_open",{profileId:profile.id,password,allowFtp});
   if(!alive.current){await invoke("transfer_close",{id});return;}
   sessionRef.current=id;setSession(id);setConnected(true);setPassword("");
   setStatus("Conexão estabelecida.");
   await refresh(id,"/");
  });
 }
 async function disconnect(){
  await withBusy(async()=>{
   const id=sessionRef.current;
   sessionRef.current="";setSession("");setConnected(false);setEntries([]);setSelected(null);
   if(id)await invoke("transfer_close",{id});
   setStatus("Conexão encerrada.");
  });
 }
 async function changeFolder(newPath:string){await withBusy(()=>refresh(sessionRef.current,newPath));}
 async function upload(){
  if(!sessionRef.current)return;
  await withBusy(async()=>{
   const local=await invoke<string|null>("pick_local_file");
   if(!local)return;
   const base=local.split(/[\\/]/).pop()||"arquivo";
   const folder=pathEntry.trim().startsWith("/")?pathEntry.trim():path;
   const dest=remoteFile.trim()||((folder==="/"?"/":folder.replace(/\/$/,"")+"/")+base);
   if(overwrite&&!confirm("Sobrescrever arquivo remoto se existir? "+dest))return;
   setStatus("Enviando "+base+" para "+dest);
   const bytes=await invoke<number>("transfer_copy",{id:sessionRef.current,remote:dest,local,upload:true,overwrite});
   setStatus("Upload concluído: "+size(bytes)+" → "+dest);
   await refresh(sessionRef.current,path);
  });
 }
 async function download(){
  if(!sessionRef.current)return;
  await withBusy(async()=>{
   const remote=selected&&!selected.directory?selected.path:remoteFile.trim();
   if(!remote){setStatus("Selecione um arquivo remoto ou informe caminho completo.");return;}
   const proposed=remote.split("/").pop()||"arquivo";
   const local=await invoke<string|null>("pick_save_file",{suggestedName:proposed});
   if(!local)return;
   if(overwrite&&!confirm("Permitir sobrescrever o arquivo local selecionado?"))return;
   setStatus("Baixando "+remote);
   const bytes=await invoke<number>("transfer_copy",{id:sessionRef.current,remote,local,upload:false,overwrite});
   setStatus("Download concluído: "+size(bytes)+" → "+local);
  });
 }
 async function mkdir(){
  const name=prompt("Nome da pasta remota (será criada no diretório atual):");
  if(!name)return;if(name.includes("/")||name==="."||name===".."){setStatus("Nome da pasta inválido");return;}
  await withBusy(async()=>{
   const remote=(path==="/"?"/":path.replace(/\/$/,"")+"/")+name;
   await invoke("transfer_mkdir",{id:sessionRef.current,path:remote});
   await refresh(sessionRef.current,path);
  });
 }
 async function remove(){
  if(!selected)return;
  if(!confirm("Apagar remotamente "+selected.path+"? Esta operação não pode ser desfeita."))return;
  await withBusy(async()=>{
   await invoke("transfer_delete",{id:sessionRef.current,path:selected.path,directory:selected.directory});
   await refresh(sessionRef.current,path);
  });
 }
 const ftp=profile.protocol==="ftp";
 const scp=profile.protocol==="scp";
 return <section className="transfer-panel">
  <div className="transfer-top">
   <strong>⇄ {profile.protocol==="ssh"?"SFTP via SSH":profile.protocol.toUpperCase()} · {profile.name}</strong>
   <span className="muted">{profile.host}:{profile.port}</span>
   {connected&&<button onClick={()=>void disconnect()} disabled={busy}>Desconectar</button>}
  </div>
  {!connected&&<div className="transfer-login">
   <h3>Autenticação {ftp?"FTP":"SSH"}</h3>
   <p>Usuário: <strong>{profile.username||"(não configurado no perfil)"}</strong></p>
   {!ftp&&<p className="muted">Sem senha: tenta ssh-agent e chaves padrão; requer host previamente confirmado em known_hosts.</p>}
   <input type="password" autoComplete="off" value={password} onChange={e=>setPassword(e.target.value)} placeholder={ftp?"Senha FTP":"Senha SSH (deixe vazio para chaves/agent)"}/>
   {ftp&&<label className="ftp-consent"><input type="checkbox" checked={allowFtp} onChange={e=>setAllowFtp(e.target.checked)}/>
     Estou ciente de que FTP transmite a senha e os arquivos sem criptografia.
   </label>}
   <button disabled={busy||!profile.username.trim()||(ftp&&(!password||!allowFtp))} onClick={()=>void connect()}>
    {busy?"Conectando...":"Conectar"}
   </button>
  </div>}
  {connected&&<>
   <div className="transfer-path">
    <button title="Pasta acima" onClick={()=>void changeFolder(parent(path))} disabled={busy||path==="/"}>↑</button>
    <input aria-label="Caminho remoto" value={pathEntry} onChange={e=>setPathEntry(e.target.value)}
       onKeyDown={e=>{if(e.key==="Enter")void changeFolder(pathEntry)}}/>
    <button disabled={busy} onClick={()=>void changeFolder(pathEntry)}>Ir</button>
    <button disabled={busy} onClick={()=>void changeFolder(path)}>↻</button>
   </div>
   <div className="transfer-list" role="table" aria-label="Arquivos remotos">
    <div className="transfer-head" role="row"><span>Nome</span><span>Tamanho</span></div>
    {entries.map(item=><button type="button" key={item.path}
      className={"transfer-row "+(selected?.path===item.path?"chosen":"")}
      onClick={()=>{setSelected(item);if(!item.directory)setRemoteFile(item.path)}}
      onDoubleClick={()=>{if(item.directory)void changeFolder(item.path);else{setSelected(item);setRemoteFile(item.path);}}}
      title={item.path}>
      <span>{item.directory?"📁":"📄"} {item.name}</span><span>{item.directory?"Pasta":size(item.size)}</span>
    </button>)}
    {entries.length===0&&<p className="muted">Nenhum item listado. {scp?"SCP pode transferir pelo caminho completo mesmo sem SFTP.":""}</p>}
   </div>
   <div className="transfer-operations">
    <label>Arquivo remoto completo<input placeholder="/home/usuario/arquivo.txt" value={remoteFile} onChange={e=>setRemoteFile(e.target.value)}/></label>
    <label className="overwrite-check"><input type="checkbox" checked={overwrite} onChange={e=>setOverwrite(e.target.checked)}/> Permitir sobrescrever arquivos</label>
    <div className="transfer-buttons">
      <button disabled={busy} onClick={()=>void upload()}>↑ Enviar arquivo</button>
      <button disabled={busy} onClick={()=>void download()}>↓ Baixar arquivo</button>
      <button disabled={busy} onClick={()=>void mkdir()}>+ Nova pasta</button>
      <button disabled={busy||!selected} onClick={()=>void remove()}>Excluir selecionado</button>
    </div>
   </div>
  </>}
  <div className="transfer-status" role="status">
   <span>{status}</span>
   {progress&&<span>{progress.operation==="upload"?"↑":"↓"} {size(progress.transferred)}{progress.total>0?" / "+size(progress.total):""}{progress.done?" — finalizado":""}</span>}
  </div>
 </section>;
}
