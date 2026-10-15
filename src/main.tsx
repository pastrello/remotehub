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
type RdpEvent = {eventId:string;phase:string;status:string;terminal:boolean};
function RdpPanel({profile,status}:{profile:Profile;status?:string}) {
const [password,setPassword]=useState("");
const [session,setSession]=useState("");
const [progress,setProgress]=useState("Nenhuma autenticação RDP confirmada.");
const [phase,setPhase]=useState("idle");
const [busy,setBusy]=useState(false);
const [listenerReady,setListenerReady]=useState(false);
const sessionRef=useRef("");
const attemptRef=useRef("");
const terminatedRef=useRef(false);
useEffect(()=>{
  let alive=true;let off:(()=>void)|undefined;
  listen<RdpEvent>("rdp-status",e=>{
    if(!alive||e.payload.eventId!==attemptRef.current)return;
    setPhase(e.payload.phase);
    setProgress(e.payload.status);
    if(e.payload.terminal){
      terminatedRef.current=true;
      sessionRef.current="";
      setSession("");
      setBusy(false);
    }
  }).then(fn=>{if(alive){off=fn;setListenerReady(true)}else fn()}).catch(()=>{
    if(alive){setListenerReady(false);setProgress("Não foi possível receber os eventos do motor RDP.");}
  });
  return()=>{alive=false;off?.();if(sessionRef.current)void invoke("rdp_close",{id:sessionRef.current}).catch(()=>{})};
},[]);
async function connect(){
  if(busy||session)return;
  if(!profile.username.trim()){setProgress("Defina o usuário no cadastro RDP.");return;}
  if(!password){setProgress("Informe a senha para autenticação.");return;}
  const attempt=crypto.randomUUID();
  attemptRef.current=attempt;
  terminatedRef.current=false;
  setBusy(true);setPhase("connecting");
  setProgress("Iniciando IronRDP com verificação estrita de certificado...");
  try{
    const id=await invoke<string>("rdp_start",{profileId:profile.id,password,eventId:attempt});
    if(!terminatedRef.current){sessionRef.current=id;setSession(id);}
  }catch(_e){
    if(!terminatedRef.current){setPhase("failed");setProgress("Falha ao iniciar o motor RDP. Verifique dados do perfil.");}
  }finally{setPassword("");setBusy(false);}
}
async function disconnect(){
  if(!sessionRef.current)return;
  const id=sessionRef.current;
  setProgress("Solicitando encerramento da sessão RDP...");
  try{await invoke("rdp_close",{id})}catch{setProgress("Falha ao solicitar encerramento RDP.");}
}
const stage={
  idle:"Aguardando conexão",
  connecting:"Conectando",
  activated:"RDP ativado, login não confirmado",
  authenticated:"Login confirmado",
  first_image:"Frame recebido",
  failed:"Falha na conexão",
  incomplete:"Encerrada antes de concluir",
  closed:"Encerrada"
}[phase]||phase;
return <section className="empty" style={{width:"100%"}}>
  <h2>RDP · {profile.name}</h2>
  <p>{profile.host}:{profile.port}</p>
  <p>{status}</p>
  <p><strong>Motor IronRDP experimental</strong> · TLS com validação estrita do certificado.</p>
  <p><strong>Estado:</strong> {stage}</p>
  {!session&&!busy?<div style={{maxWidth:330,display:"grid",gap:8}}>
    <p>Usuário: {profile.username||"(não configurado)"}</p>
    <input type="password" autoComplete="off" placeholder="Senha RDP (não será salva)" value={password} onChange={e=>setPassword(e.target.value)}/>
    <button disabled={!listenerReady} onClick={()=>void connect()}>Conectar com IronRDP</button>
  </div>:<button disabled={!session} onClick={()=>void disconnect()}>{session?"Desconectar RDP":"Iniciando sessão..."}</button>}
  <p role="status">{progress}</p>
  <p className="muted">Login só é confirmado após o evento LoginComplete. Sem renderização gráfica nesta versão. Use apenas credenciais de laboratório.</p>
</section>;
}
function App(){const[profiles,setProfiles]=useState<Profile[]>([]);const[form,setForm]=useState({...empty,id:""});const[tabs,setTabs]=useState<Tab[]>([]);const[active,setActive]=useState("");const[error,setError]=useState("");const[search,setSearch]=useState("");
async function refresh(){try{setProfiles(await invoke<Profile[]>("profiles_list"))}catch(e){setError(String(e))}}
useEffect(()=>{refresh()},[]);
async function save(e:React.FormEvent){e.preventDefault();try{await invoke("profiles_save",{input:{...form,id:form.id||null,port:Number(form.port)}});setForm({...empty,id:""});await refresh()}catch(e){setError(String(e))}}
async function open(p:Profile){setError("");if(p.protocol==="rdp"){const id=crypto.randomUUID();setTabs(prev=>[...prev,{id,profile:p,rdpStatus:"Verificando conexão TCP..."}]);setActive(id);invoke<{address:string;security:string;note:string}>("rdp_negotiate",{profileId:p.id}).then(result=>setTabs(prev=>prev.map(t=>t.id===id?{...t,rdpStatus:result.security+" — "+result.note}:t))).catch(e=>setTabs(prev=>prev.map(t=>t.id===id?{...t,rdpStatus:String(e)}:t)));return}try{const sid=await invoke<string>("ssh_open",{profileId:p.id});const id=crypto.randomUUID();setTabs(prev=>[...prev,{id,profile:p,sid}]);setActive(id)}catch(e){setError(String(e))}}
async function close(t:Tab){if(t.sid)await invoke("ssh_close",{id:t.sid}).catch(console.error);setTabs(prev=>prev.filter(x=>x.id!==t.id));if(active===t.id)setActive("")}
return <div className="app"><aside className="sidebar"><h2>RemoteHub <small className="muted">0.0.3-dev</small></h2><p className="muted">SSH validado · IronRDP experimental</p><input placeholder="Buscar..." value={search} onChange={e=>setSearch(e.target.value)}/><h3>Conexões</h3>{profiles.filter(p=>(p.name+" "+p.host+" "+p.folder).toLowerCase().includes(search.toLowerCase())).map(p=><div key={p.id}><button className="item" onClick={()=>open(p)}>{p.protocol.toUpperCase()} · {p.name}<span className="muted"> ({p.host})</span></button><div className="row"><button onClick={()=>setForm({...p})}>Editar</button><button onClick={async()=>{if(confirm("Excluir perfil?")){await invoke("profiles_delete",{id:p.id});refresh()}}}>Excluir</button></div></div>)}<h3>{form.id?"Editar":"Nova"} conexão</h3><form className="form" onSubmit={save}><input required placeholder="Nome" value={form.name} onChange={e=>setForm({...form,name:e.target.value})}/><select value={form.protocol} onChange={e=>setForm({...form,protocol:e.target.value,port:e.target.value==="rdp"?3389:22})}><option value="ssh">SSH</option><option value="rdp">RDP (IronRDP experimental)</option></select><input required placeholder="Host" value={form.host} onChange={e=>setForm({...form,host:e.target.value})}/><input required type="number" min="1" max="65535" value={form.port} onChange={e=>setForm({...form,port:Number(e.target.value)})}/><input placeholder="Usuário" value={form.username} onChange={e=>setForm({...form,username:e.target.value})}/><input placeholder="Pasta" value={form.folder} onChange={e=>setForm({...form,folder:e.target.value})}/><button type="submit">Salvar perfil</button></form></aside><main className="work"><div className="tabs">{tabs.map(t=><button className={"tab "+(active===t.id?"selected":"")} key={t.id} onClick={()=>setActive(t.id)}>{t.profile.name} <span onClick={e=>{e.stopPropagation();close(t)}}> ×</span></button>)}</div>{error&&<div className="banner">{error} <button onClick={()=>setError("")}>×</button></div>}{tabs.length===0&&<div className="empty">Selecione SSH para conectar ou RDP para testar acessibilidade TCP.</div>}{tabs.map(t=><div key={t.id} style={{display:active===t.id?"flex":"none",flex:1,minHeight:0}}>{t.sid?<Shell sid={t.sid}/>:<RdpPanel profile={t.profile} status={t.rdpStatus}/>}</div>)}</main></div>}
createRoot(document.getElementById("root")!).render(<App/>);
