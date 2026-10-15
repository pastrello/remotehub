import React,{useEffect,useRef,useState} from "react";
import{createRoot}from"react-dom/client";
import{invoke}from"@tauri-apps/api/core";
import{listen}from"@tauri-apps/api/event";
import{Terminal}from"@xterm/xterm";
import{FitAddon}from"@xterm/addon-fit";
import "./style.css";
type Profile={id:string;name:string;protocol:string;host:string;port:number;username:string;folder:string};
type Tab={id:string;profile:Profile;sid?:string;rdpStatus?:string};
const empty={name:"",protocol:"ssh",host:"",port:22,username:"",folder:"Geral"};
function b64(s:string){return btoa(String.fromCharCode(...new TextEncoder().encode(s)))}
function unb64(s:string){return new TextDecoder().decode(Uint8Array.from(atob(s),c=>c.charCodeAt(0)))}
function Shell({sid}:{sid:string}){const ref=useRef<HTMLDivElement>(null);useEffect(()=>{if(!ref.current)return;const term=new Terminal({cursorBlink:true,theme:{background:"#111827"}});const fit=new FitAddon();term.loadAddon(fit);term.open(ref.current);fit.fit();term.focus();const input=term.onData(data=>{invoke("ssh_write",{id:sid,data:b64(data)}).catch(console.error)});let active=true;const offs: Array<()=>void>=[];listen<{id:string,data:string}>("ssh-output",e=>{if(active&&e.payload.id===sid)term.write(unb64(e.payload.data))}).then(f=>{if(active)offs.push(f);else f()});listen<{id:string}>("ssh-closed",e=>{if(active&&e.payload.id===sid)term.writeln("\r\n[SSH encerrado]")}).then(f=>{if(active)offs.push(f);else f()});const resize=()=>{fit.fit();invoke("ssh_resize",{id:sid,rows:term.rows,cols:term.cols}).catch(()=>{})};window.addEventListener("resize",resize);return()=>{active=false;offs.forEach(f=>f());window.removeEventListener("resize",resize);input.dispose();term.dispose()};},[sid]);return <div className="terminal" ref={ref}/> }
function RdpPanel({profile,status}:{profile:Profile;status?:string}) {
const [password,setPassword]=useState("");
const [session,setSession]=useState("");
const [progress,setProgress]=useState("");
const [busy,setBusy]=useState(false);
const sessionRef=useRef("");
useEffect(()=>{let alive=true;let off:(()=>void)|undefined;
listen<{id:string;status:string}>("rdp-status",e=>{if(alive&&e.payload.id===sessionRef.current)setProgress(e.payload.status)}).then(fn=>{if(alive)off=fn;else fn()});
return()=>{alive=false;off?.();if(sessionRef.current)void invoke("rdp_close",{id:sessionRef.current}).catch(()=>{})};
},[]);
async function connect(){
if(!profile.username.trim()){setProgress("Defina o usuário no cadastro RDP.");return;}
if(!password){setProgress("Informe a senha para autenticação.");return;}
setBusy(true);setProgress("Iniciando RDP com validação estrita de certificado...");
try{const id=await invoke<string>("rdp_start",{profileId:profile.id,password});
sessionRef.current=id;setSession(id);setProgress("RDP iniciado; aguardando negociação TLS/NLA.");
}catch(_e){setProgress("Não foi possível iniciar sessão. Verifique configuração e certificados.");}
finally{setPassword("");setBusy(false)}
}
async function disconnect(){if(sessionRef.current)await invoke("rdp_close",{id:sessionRef.current}).catch(()=>{});sessionRef.current="";setSession("");setProgress("Desconexão solicitada.");}
return <section className="empty" style={{width:"100%"}}><h2>RDP · {profile.name}</h2><p>{profile.host}:{profile.port}</p><p>{status}</p>
<p><strong>Motor IronRDP experimental</strong> · Certificado TLS verificado estritamente.</p>
{!session?<div style={{maxWidth:330,display:"grid",gap:8}}><p>Usuário: {profile.username||"(não configurado)"}</p>
<input type="password" autoComplete="off" placeholder="Senha RDP (não será salva)" value={password} onChange={e=>setPassword(e.target.value)} />
<button disabled={busy} onClick={()=>void connect()}>{busy?"Conectando...":"Conectar com IronRDP"}</button></div>:<button onClick={()=>void disconnect()}>Desconectar RDP</button>}
<p>{progress}</p><p className="muted">Ainda não renderizamos a imagem do desktop nesta aba. Não utilize credenciais de produção na PoC.</p></section>
}
function App(){const[profiles,setProfiles]=useState<Profile[]>([]);const[form,setForm]=useState({...empty,id:""});const[tabs,setTabs]=useState<Tab[]>([]);const[active,setActive]=useState("");const[error,setError]=useState("");const[search,setSearch]=useState("");
async function refresh(){try{setProfiles(await invoke<Profile[]>("profiles_list"))}catch(e){setError(String(e))}}
useEffect(()=>{refresh()},[]);
async function save(e:React.FormEvent){e.preventDefault();try{await invoke("profiles_save",{input:{...form,id:form.id||null,port:Number(form.port)}});setForm({...empty,id:""});await refresh()}catch(e){setError(String(e))}}
async function open(p:Profile){setError("");if(p.protocol==="rdp"){const id=crypto.randomUUID();setTabs(prev=>[...prev,{id,profile:p,rdpStatus:"Verificando conexão TCP..."}]);setActive(id);invoke<{address:string;security:string;note:string}>("rdp_negotiate",{profileId:p.id}).then(result=>setTabs(prev=>prev.map(t=>t.id===id?{...t,rdpStatus:result.security+" — "+result.note}:t))).catch(e=>setTabs(prev=>prev.map(t=>t.id===id?{...t,rdpStatus:String(e)}:t)));return}try{const sid=await invoke<string>("ssh_open",{profileId:p.id});const id=crypto.randomUUID();setTabs(prev=>[...prev,{id,profile:p,sid}]);setActive(id)}catch(e){setError(String(e))}}
async function close(t:Tab){if(t.sid)await invoke("ssh_close",{id:t.sid}).catch(console.error);setTabs(prev=>prev.filter(x=>x.id!==t.id));if(active===t.id)setActive("")}
return <div className="app"><aside className="sidebar"><h2>RemoteHub <small className="muted">0.0.3-dev</small></h2><p className="muted">SSH validado · IronRDP experimental</p><input placeholder="Buscar..." value={search} onChange={e=>setSearch(e.target.value)}/><h3>Conexões</h3>{profiles.filter(p=>(p.name+" "+p.host+" "+p.folder).toLowerCase().includes(search.toLowerCase())).map(p=><div key={p.id}><button className="item" onClick={()=>open(p)}>{p.protocol.toUpperCase()} · {p.name}<span className="muted"> ({p.host})</span></button><div className="row"><button onClick={()=>setForm({...p})}>Editar</button><button onClick={async()=>{if(confirm("Excluir perfil?")){await invoke("profiles_delete",{id:p.id});refresh()}}}>Excluir</button></div></div>)}<h3>{form.id?"Editar":"Nova"} conexão</h3><form className="form" onSubmit={save}><input required placeholder="Nome" value={form.name} onChange={e=>setForm({...form,name:e.target.value})}/><select value={form.protocol} onChange={e=>setForm({...form,protocol:e.target.value,port:e.target.value==="rdp"?3389:22})}><option value="ssh">SSH</option><option value="rdp">RDP (IronRDP experimental)</option></select><input required placeholder="Host" value={form.host} onChange={e=>setForm({...form,host:e.target.value})}/><input required type="number" min="1" max="65535" value={form.port} onChange={e=>setForm({...form,port:Number(e.target.value)})}/><input placeholder="Usuário" value={form.username} onChange={e=>setForm({...form,username:e.target.value})}/><input placeholder="Pasta" value={form.folder} onChange={e=>setForm({...form,folder:e.target.value})}/><button type="submit">Salvar perfil</button></form></aside><main className="work"><div className="tabs">{tabs.map(t=><button className={"tab "+(active===t.id?"selected":"")} key={t.id} onClick={()=>setActive(t.id)}>{t.profile.name} <span onClick={e=>{e.stopPropagation();close(t)}}> ×</span></button>)}</div>{error&&<div className="banner">{error} <button onClick={()=>setError("")}>×</button></div>}{tabs.length===0&&<div className="empty">Selecione SSH para conectar ou RDP para testar acessibilidade TCP.</div>}{tabs.map(t=><div key={t.id} style={{display:active===t.id?"flex":"none",flex:1,minHeight:0}}>{t.sid?<Shell sid={t.sid}/>:<RdpPanel profile={t.profile} status={t.rdpStatus}/>}</div>)}</main></div>}
createRoot(document.getElementById("root")!).render(<App/>);
