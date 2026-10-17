import React,{useEffect,useMemo,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
export type Profile={id:string;name:string;protocol:string;host:string;port:number;username:string;folder:string};
type Props={profiles:Profile[];refresh:()=>Promise<void>;onOpen:(p:Profile)=>void;onOpenFiles:(p:Profile)=>void;onError:(s:string)=>void};
const empty={id:"",name:"",protocol:"ssh",host:"",port:22,username:"",folder:"Geral"};
const ports:Record<string,number>={ssh:22,rdp:3389,sftp:22,scp:22,ftp:21};
function basename(path:string){return path.slice(path.lastIndexOf("/")+1);}
function ancestors(path:string){
  const parts=path.split("/"),arr:string[]=[];
  for(let i=0;i<parts.length;i++)arr.push(parts.slice(0,i+1).join("/"));
  return arr;
}
export function ConnectionSidebar({profiles,refresh,onOpen,onOpenFiles,onError}:Props){
 const[search,setSearch]=useState("");
 const[folders,setFolders]=useState<string[]>([]);
 const[expanded,setExpanded]=useState<Set<string>>(new Set(["Geral"]));
 const[menu,setMenu]=useState<{x:number;y:number;type:"folder"|"profile"|"blank";path?:string;profile?:Profile}|null>(null);
 const[editing,setEditing]=useState<typeof empty|null>(null);
 const[folderDialog,setFolderDialog]=useState<{mode:"new"|"rename";path:string}|null>(null);
 const[folderValue,setFolderValue]=useState("");
 async function reload(){
  await refresh();
  try{const all=await invoke<string[]>("folders_list");setFolders(all);setExpanded(prev=>new Set([...prev,...all]));}
  catch(e){onError(String(e));}
 }
 useEffect(()=>{void invoke<string[]>("folders_list").then(all=>{setFolders(all);setExpanded(new Set(all));}).catch(e=>onError(String(e)));},[]);
 useEffect(()=>{
  const hide=()=>setMenu(null);
  const esc=(e:KeyboardEvent)=>{if(e.key==="Escape")setMenu(null);};
  document.addEventListener("click",hide);
  document.addEventListener("keydown",esc);
  return()=>{document.removeEventListener("click",hide);document.removeEventListener("keydown",esc);};
 },[]);
 const allFolders=useMemo(()=>{
  const set=new Set<string>(folders);
  for(const p of profiles){
    for(const name of ancestors(p.folder||"Geral"))set.add(name);
  }
  if(!set.size)set.add("Geral");
  return Array.from(set).sort((a,b)=>a.localeCompare(b,"pt-BR",{sensitivity:"base"}));
 },[folders,profiles]);
 const matching=useMemo(()=>profiles.filter(p=>
   (p.name+" "+p.host+" "+p.folder+" "+p.protocol).toLowerCase().includes(search.toLowerCase())),[search,profiles]);
 const visibleFolders=useMemo(()=>{
  const results=new Set<string>();
  for(const p of matching){ancestors(p.folder||"Geral").forEach(x=>results.add(x));}
  for(const path of allFolders){
   if(!search||path.toLowerCase().includes(search.toLowerCase()))ancestors(path).forEach(x=>results.add(x));
  }
  return allFolders.filter(x=>results.has(x));
 },[matching,allFolders,search]);
 function context(e:React.MouseEvent,info:NonNullable<typeof menu>){
  e.preventDefault();e.stopPropagation();
  setMenu({...info,x:Math.min(e.clientX,window.innerWidth-225),y:Math.min(e.clientY,window.innerHeight-290)});
 }
 function startFolder(mode:"new"|"rename",path:string){setMenu(null);setFolderDialog({mode,path});setFolderValue(mode==="new"?"":basename(path));}
 async function saveFolder(e:React.FormEvent){
  e.preventDefault();const value=folderValue.trim();
  if(!value||value.includes("/")||value==="."||value===".."){onError("Nome de pasta inválido");return;}
  const model=folderDialog;if(!model)return;
  const parent=model.mode==="new"?model.path:model.path.split("/").slice(0,-1).join("/");
  const path=parent?parent+"/"+value:value;
  try {
   if(model.mode==="new")await invoke("folder_create",{path});
   else await invoke("folder_rename",{oldPath:model.path,newPath:path});
   setFolderDialog(null);await reload();
  }catch(e){onError(String(e))}
 }
 async function removeFolder(path:string){
  if(!window.confirm("Excluir pasta vazia '"+path+"'?"))return;
  try{await invoke("folder_delete",{path});await reload()}catch(e){onError(String(e))}
 }
 async function removeProfile(p:Profile){
  if(!confirm("Apagar perfil '"+p.name+"'? A operação não encerra sessões já abertas."))return;
  try{await invoke("profiles_delete",{id:p.id});await reload()}catch(e){onError(String(e))}
 }
 async function cloneProfile(p:Profile){
  try{await invoke("profile_clone",{id:p.id});await reload()}catch(e){onError(String(e))}
 }
 async function save(e:React.FormEvent){
  e.preventDefault();if(!editing)return;
  try{await invoke("profiles_save",{input:{...editing,id:editing.id||null,port:Number(editing.port),folder:editing.folder.trim()||"Geral"}});
   setEditing(null);await reload();
  }catch(e){onError(String(e))}
 }
 function newProfile(folder:string){setMenu(null);setEditing({...empty,folder});}
 function toggle(path:string){
  setExpanded(prev=>{const out=new Set(prev);if(out.has(path))out.delete(path);else out.add(path);return out;});
 }
 const rootFolders=visibleFolders.filter(p=>!p.includes("/"));
 function renderFolder(path:string,depth:number):React.ReactNode {
  const children=visibleFolders.filter(p=>p.startsWith(path+"/")&&p.split("/").length===path.split("/").length+1);
  const items=matching.filter(p=>(p.folder||"Geral")===path);
  const open=search!==""||expanded.has(path);
  return <div key={"dir:"+path}>
   <div className="tree-folder" style={{paddingLeft:8+depth*17}} onContextMenu={e=>context(e,{x:0,y:0,type:"folder",path})}>
     <button className="tree-chevron" aria-label={open?"Recolher":"Expandir"} onClick={()=>toggle(path)}>{open?"▾":"▸"}</button>
     <button className="tree-folder-title" onClick={()=>toggle(path)} title={path}><span className="folder-icon">📁</span>{basename(path)}</button>
   </div>
   {open&&<div>
      {children.map(p=>renderFolder(p,depth+1))}
      {items.map(p=><div className="tree-profile" style={{paddingLeft:23+(depth+1)*17}} key={p.id}
        onContextMenu={e=>context(e,{x:0,y:0,type:"profile",profile:p})}
        onDoubleClick={()=>onOpen(p)}>
        <span className={"protocol-dot protocol-"+p.protocol}>{p.protocol==="ssh"?"⌁":p.protocol==="rdp"?"▣":"⇅"}</span>
        <button title={p.protocol.toUpperCase()+" · "+p.host+":"+p.port} onClick={()=>onOpen(p)}>{p.name}</button>
      </div>)}
   </div>}
  </div>;
 }
 return <aside className="sidebar connection-sidebar" onContextMenu={e=>context(e,{x:0,y:0,type:"blank"})}>
  <header className="sidebar-heading"><div><strong>RemoteHub</strong> <small>0.0.6-dev</small></div><button title="Adicionar conexão" onClick={()=>newProfile("Geral")}>＋</button></header>
  <input aria-label="Buscar conexões" placeholder="Buscar conexões..." value={search} onChange={e=>setSearch(e.target.value)}/>
  <div className="tree-heading"><span>★ Sessões e servidores</span><button title="Nova pasta" onClick={()=>startFolder("new","")}>+ Pasta</button></div>
  <div className="connection-tree">{rootFolders.map(p=>renderFolder(p,0))}</div>
  <div className="sidebar-foot">Duplo clique para abrir · botão direito para ações</div>
  {menu&&<div className="context-menu" role="menu" style={{left:menu.x,top:menu.y}} onClick={e=>e.stopPropagation()}>
    {menu.type==="profile"&&menu.profile&&<>
      <button onClick={()=>{onOpen(menu.profile!);setMenu(null)}}>▶ Executar</button>
      {(menu.profile.protocol==="ssh"||["sftp","scp","ftp"].includes(menu.profile.protocol))&&<button onClick={()=>{onOpenFiles(menu.profile!);setMenu(null)}}>⇄ Transferir arquivos</button>}
      <button onClick={()=>{setEditing({...menu.profile!});setMenu(null)}}>✎ Editar</button>
      <button onClick={()=>{void cloneProfile(menu.profile!);setMenu(null)}}>⧉ Clonar</button>
      <hr/>
      <button className="danger" onClick={()=>{void removeProfile(menu.profile!);setMenu(null)}}>✕ Apagar</button>
    </>}
    {menu.type==="folder"&&<><button onClick={()=>newProfile(menu.path||"Geral")}>＋ Nova conexão nesta pasta</button>
      <button onClick={()=>startFolder("new",menu.path||"")}>📁 Nova subpasta</button>
      <button onClick={()=>startFolder("rename",menu.path||"")}>✎ Renomear pasta</button>
      <hr/><button className="danger" onClick={()=>{void removeFolder(menu.path||"");setMenu(null)}}>✕ Excluir pasta vazia</button></>}
    {menu.type==="blank"&&<><button onClick={()=>newProfile("Geral")}>＋ Nova conexão</button><button onClick={()=>startFolder("new","")}>📁 Nova pasta</button></>}
  </div>}
  {editing&&<div className="modal-scrim" onMouseDown={e=>{if(e.target===e.currentTarget)setEditing(null)}}>
    <form className="editor-modal" onSubmit={save} onMouseDown={e=>e.stopPropagation()}>
      <h2>{editing.id?"Editar conexão":"Nova conexão"}</h2>
      <label>Nome<input required value={editing.name} onChange={e=>setEditing({...editing,name:e.target.value})}/></label>
      <label>Protocolo<select value={editing.protocol} onChange={e=>{const p=e.target.value;setEditing({...editing,protocol:p,port:ports[p]||22})}}>
        <option value="ssh">SSH — Terminal</option><option value="sftp">SFTP — Arquivos via SSH</option>
        <option value="scp">SCP — Cópia segura SSH</option><option value="ftp">FTP — Sem criptografia</option>
        <option value="rdp">RDP — Experimental (congelado)</option>
      </select></label>
      <div className="editor-two"><label>Servidor<input required value={editing.host} onChange={e=>setEditing({...editing,host:e.target.value})}/></label>
        <label>Porta<input required type="number" min="1" max="65535" value={editing.port} onChange={e=>setEditing({...editing,port:Number(e.target.value)})}/></label></div>
      <label>Usuário<input value={editing.username} onChange={e=>setEditing({...editing,username:e.target.value})}/></label>
      <label>Pasta<input value={editing.folder} onChange={e=>setEditing({...editing,folder:e.target.value})} placeholder="Geral/Servidores"/></label>
      {editing.protocol==="ftp"&&<p className="ftp-warning">FTP clássico não criptografa senha nem arquivos. Prefira SFTP.</p>}
      <p className="muted">Credenciais não são armazenadas. SSH usa as chaves conhecidas do OpenSSH.</p>
      <div className="editor-actions"><button type="button" onClick={()=>setEditing(null)}>Cancelar</button><button type="submit">Salvar perfil</button></div>
    </form>
  </div>}
  {folderDialog&&<div className="modal-scrim"><form className="editor-modal folder-dialog" onSubmit={saveFolder}>
    <h2>{folderDialog.mode==="new"?"Nova pasta":"Renomear pasta"}</h2>
    <p className="muted">{folderDialog.mode==="new"?(folderDialog.path||"Raiz"):folderDialog.path}</p>
    <input required autoFocus placeholder="Nome da pasta" value={folderValue} onChange={e=>setFolderValue(e.target.value)}/>
    <div className="editor-actions"><button type="button" onClick={()=>setFolderDialog(null)}>Cancelar</button><button type="submit">Confirmar</button></div>
  </form></div>}
 </aside>;
}
