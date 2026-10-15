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
type RdpEvent={eventId:string;phase:string;status:string;terminal:boolean};
type RdpFrame={eventId:string;sequence:number;width:number;height:number;jpegBase64:string};
type RdpInput=
  |{kind:"key";scancode:number;extended:boolean;pressed:boolean}
  |{kind:"mouse";action:"move"|"down"|"up"|"wheel";x:number;y:number;button:number;wheel:number};
// Physical set-1 scan codes. Browser KeyboardEvent.code identifies positions, not characters.
// Dead keys, IMEs and layout-specific mappings will be extended in a later version.
const keyMap:Record<string,[number,boolean]>={
  Escape:[0x01,false],Digit1:[0x02,false],Digit2:[0x03,false],Digit3:[0x04,false],
  Digit4:[0x05,false],Digit5:[0x06,false],Digit6:[0x07,false],Digit7:[0x08,false],
  Digit8:[0x09,false],Digit9:[0x0a,false],Digit0:[0x0b,false],Minus:[0x0c,false],
  Equal:[0x0d,false],Backspace:[0x0e,false],Tab:[0x0f,false],KeyQ:[0x10,false],
  KeyW:[0x11,false],KeyE:[0x12,false],KeyR:[0x13,false],KeyT:[0x14,false],
  KeyY:[0x15,false],KeyU:[0x16,false],KeyI:[0x17,false],KeyO:[0x18,false],
  KeyP:[0x19,false],BracketLeft:[0x1a,false],BracketRight:[0x1b,false],
  Enter:[0x1c,false],ControlLeft:[0x1d,false],KeyA:[0x1e,false],
  KeyS:[0x1f,false],KeyD:[0x20,false],KeyF:[0x21,false],KeyG:[0x22,false],
  KeyH:[0x23,false],KeyJ:[0x24,false],KeyK:[0x25,false],KeyL:[0x26,false],
  Semicolon:[0x27,false],Quote:[0x28,false],Backquote:[0x29,false],
  ShiftLeft:[0x2a,false],Backslash:[0x2b,false],KeyZ:[0x2c,false],
  KeyX:[0x2d,false],KeyC:[0x2e,false],KeyV:[0x2f,false],KeyB:[0x30,false],
  KeyN:[0x31,false],KeyM:[0x32,false],Comma:[0x33,false],Period:[0x34,false],
  Slash:[0x35,false],ShiftRight:[0x36,false],NumpadMultiply:[0x37,false],
  AltLeft:[0x38,false],Space:[0x39,false],CapsLock:[0x3a,false],
  F1:[0x3b,false],F2:[0x3c,false],F3:[0x3d,false],F4:[0x3e,false],
  F5:[0x3f,false],F6:[0x40,false],F7:[0x41,false],F8:[0x42,false],
  F9:[0x43,false],F10:[0x44,false],NumLock:[0x45,false],ScrollLock:[0x46,false],
  Numpad7:[0x47,false],Numpad8:[0x48,false],Numpad9:[0x49,false],
  NumpadSubtract:[0x4a,false],Numpad4:[0x4b,false],Numpad5:[0x4c,false],
  Numpad6:[0x4d,false],NumpadAdd:[0x4e,false],Numpad1:[0x4f,false],
  Numpad2:[0x50,false],Numpad3:[0x51,false],Numpad0:[0x52,false],
  NumpadDecimal:[0x53,false],F11:[0x57,false],F12:[0x58,false],
  NumpadEnter:[0x1c,true],ControlRight:[0x1d,true],AltRight:[0x38,true],
  NumpadDivide:[0x35,true],ArrowUp:[0x48,true],ArrowDown:[0x50,true],
  ArrowLeft:[0x4b,true],ArrowRight:[0x4d,true],Home:[0x47,true],
  End:[0x4f,true],PageUp:[0x49,true],PageDown:[0x51,true],
  Insert:[0x52,true],Delete:[0x53,true],MetaLeft:[0x5b,true],MetaRight:[0x5c,true],
  IntlBackslash:[0x56,false],IntlRo:[0x73,false]
};

function RdpPanel({profile,status,visible}:{profile:Profile;status?:string;visible:boolean}) {
const [password,setPassword]=useState("");
const [allowInvalidCertificate,setAllowInvalidCertificate]=useState(false);
const [session,setSession]=useState("");
const [progress,setProgress]=useState("Nenhuma autenticação RDP confirmada.");
const [phase,setPhase]=useState("idle");
const [busy,setBusy]=useState(false);
const [listenerReady,setListenerReady]=useState(false);
const [resolution,setResolution]=useState("");
const [hasFrame,setHasFrame]=useState(false);
const canvasRef=useRef<HTMLCanvasElement>(null);
const sessionRef=useRef("");
const attemptRef=useRef("");
const terminatedRef=useRef(false);
const visibleRef=useRef(visible);
const activeRef=useRef(true);
const latestFrameRef=useRef<RdpFrame|null>(null);
const drawingRef=useRef(false);
const lastSequenceRef=useRef(0);
const keysRef=useRef(new Set<string>());
const lastMoveRef=useRef(0);
const buttonsRef=useRef(new Set<number>());

function sendInput(input:RdpInput){
  const id=sessionRef.current;
  if(!id||!activeRef.current)return;
  void invoke("rdp_input",{id,input}).catch(()=>{});
}
function releaseKeys(){
  for(const code of keysRef.current){
    const scan=keyMap[code];
    if(scan)sendInput({kind:"key",scancode:scan[0],extended:scan[1],pressed:false});
  }
  keysRef.current.clear();
  buttonsRef.current.clear();
}
async function drawLatest(){
  if(drawingRef.current||!visibleRef.current||!activeRef.current)return;
  drawingRef.current=true;
  try{
    while(latestFrameRef.current&&visibleRef.current&&activeRef.current){
      const frame=latestFrameRef.current;
      latestFrameRef.current=null;
      if(frame.eventId!==attemptRef.current||frame.sequence<=lastSequenceRef.current)continue;
      const bytes=Uint8Array.from(atob(frame.jpegBase64),char=>char.charCodeAt(0));
      const bitmap=await createImageBitmap(new Blob([bytes],{type:"image/jpeg"}));
      if(!activeRef.current||frame.eventId!==attemptRef.current){bitmap.close();break;}
      const canvas=canvasRef.current;
      if(canvas){
        if(canvas.width!==frame.width)canvas.width=frame.width;
        if(canvas.height!==frame.height)canvas.height=frame.height;
        canvas.getContext("2d",{alpha:false})?.drawImage(bitmap,0,0);
        lastSequenceRef.current=frame.sequence;
        setResolution(frame.width+" × "+frame.height);
        setHasFrame(true);
      }
      bitmap.close();
    }
  }catch(_err){
    setProgress("Erro ao decodificar frame gráfico recebido do IronRDP.");
  }finally{
    drawingRef.current=false;
  }
}
useEffect(()=>{
  visibleRef.current=visible;
  if(visible)void drawLatest();
  else releaseKeys();
},[visible]);
useEffect(()=>{
  let alive=true;
  activeRef.current=true;
  const offs:Array<()=>void>=[];
  Promise.all([
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
    }),
    listen<RdpFrame>("rdp-frame",e=>{
      const frame=e.payload;
      if(!alive||frame.eventId!==attemptRef.current)return;
      latestFrameRef.current=frame;
      if(visibleRef.current)void drawLatest();
    }),
  ]).then(unsubs=>{
    if(alive){offs.push(...unsubs);setListenerReady(true);}
    else unsubs.forEach(f=>f());
  }).catch(()=>{if(alive)setProgress("Falha ao registrar eventos gráficos RDP.");});
  return()=>{
    releaseKeys();
    activeRef.current=false;
    alive=false;
    offs.forEach(f=>f());
    if(sessionRef.current)void invoke("rdp_close",{id:sessionRef.current}).catch(()=>{});
  };
},[]);
async function connect(){
  if(busy||session)return;
  if(!profile.username.trim()){setProgress("Defina o usuário no cadastro RDP.");return;}
  if(!password){setProgress("Informe a senha para autenticação.");return;}
  if(allowInvalidCertificate&&!window.confirm(
    "ATENÇÃO: o certificado de "+profile.host+" não será validado nesta conexão. "+
    "Isso permite falsificação de servidor e interceptação das credenciais RDP. "+
    "Continue somente se você confia na rede e no destino. Deseja conectar?"
  ))return;
  const attempt=crypto.randomUUID();
  attemptRef.current=attempt;
  terminatedRef.current=false;
  lastSequenceRef.current=0;
  latestFrameRef.current=null;
  setHasFrame(false);setResolution("");
  canvasRef.current?.getContext("2d")?.clearRect(0,0,canvasRef.current.width,canvasRef.current.height);
  setBusy(true);setPhase("connecting");
  setProgress(allowInvalidCertificate?
    "Iniciando IronRDP sem verificar a identidade TLS do servidor...":
    "Iniciando IronRDP com verificação estrita de certificado...");
  try{
    const id=await invoke<string>("rdp_start",{profileId:profile.id,password,eventId:attempt,allowInvalidCertificate});
    // A user can close the tab before rdp_start returns. Never leak that session.
    if(!activeRef.current){
      await invoke("rdp_close",{id}).catch(()=>{});
      return;
    }
    if(!terminatedRef.current){sessionRef.current=id;setSession(id);}
  }catch(_e){
    if(!terminatedRef.current){setPhase("failed");setProgress("Falha ao iniciar o motor RDP. Verifique o perfil.");}
  }finally{setPassword("");setBusy(false);}
}
async function disconnect(){
  if(!sessionRef.current)return;
  releaseKeys();
  setProgress("Solicitando encerramento RDP...");
  try{await invoke("rdp_close",{id:sessionRef.current});}catch{setProgress("Falha ao encerrar RDP.");}
}
function coords(e:React.PointerEvent<HTMLCanvasElement>|React.WheelEvent<HTMLCanvasElement>){
  const rect=e.currentTarget.getBoundingClientRect();
  const width=e.currentTarget.width,height=e.currentTarget.height;
  const x=Math.max(0,Math.min(width-1,Math.round((e.clientX-rect.left)*width/rect.width)));
  const y=Math.max(0,Math.min(height-1,Math.round((e.clientY-rect.top)*height/rect.height)));
  return {x,y};
}
function pointer(e:React.PointerEvent<HTMLCanvasElement>,action:"move"|"down"|"up"){
  if(!sessionRef.current||!hasFrame)return;
  if(action==="move"){
    const now=Date.now();if(now-lastMoveRef.current<24)return;lastMoveRef.current=now;
  }
  if(action==="down"){e.currentTarget.focus();e.currentTarget.setPointerCapture(e.pointerId);buttonsRef.current.add(e.button);}
  if(action==="up"){buttonsRef.current.delete(e.button);if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);}
  const {x,y}=coords(e);
  sendInput({kind:"mouse",action,x,y,button:e.button,wheel:0});
}
function wheel(e:React.WheelEvent<HTMLCanvasElement>){
  if(!sessionRef.current||!hasFrame)return;
  e.preventDefault();const {x,y}=coords(e);
  sendInput({kind:"mouse",action:"wheel",x,y,button:0,wheel:e.deltaY>0?-120:120});
}
function key(e:React.KeyboardEvent<HTMLCanvasElement>,pressed:boolean){
  const scan=keyMap[e.code];if(!scan||!sessionRef.current)return;
  e.preventDefault();
  if(pressed){
    if(keysRef.current.has(e.code))return;
    keysRef.current.add(e.code);
  }else{keysRef.current.delete(e.code);}
  sendInput({kind:"key",scancode:scan[0],extended:scan[1],pressed});
}
const stage:Record<string,string>={
  idle:"Aguardando conexão",connecting:"Conectando",
  activated:"RDP ativado",authenticated:"Login confirmado",
  first_image:"Imagem recebida",failed:"Falha na conexão",
  incomplete:"Encerrada antes de concluir",closed:"Encerrada"
};
return <section className="rdp-panel">
  <div className="rdp-toolbar">
    <strong>RDP · {profile.name}</strong>
    <span className="muted">{profile.host}:{profile.port}</span>
    <span className="muted">{stage[phase]||phase}</span>
    {resolution&&<span className="muted">{resolution}</span>}
    {session&&<button onClick={()=>void disconnect()}>Desconectar</button>}
  </div>
  {!session&&!busy&&<div className="rdp-login">
    <p>{status}</p>
    <p>Usuário: {profile.username||"(não configurado)"}</p>
    <input type="password" autoComplete="off" placeholder="Senha RDP (não será salva)" value={password} onChange={e=>setPassword(e.target.value)}/>
    <label style={{display:"flex",gap:8,alignItems:"center"}}>
      <input type="checkbox" style={{width:"auto"}} checked={allowInvalidCertificate} onChange={e=>setAllowInvalidCertificate(e.target.checked)}/>
      Aceitar certificado inválido (somente nesta tentativa)
    </label>
    {allowInvalidCertificate&&<p className="rdp-warning">Identidade do servidor não será verificada: risco de interceptação das credenciais.</p>}
    <button disabled={!listenerReady} onClick={()=>void connect()}>Conectar com IronRDP</button>
  </div>}
  <div className="rdp-screen">
    <canvas ref={canvasRef} tabIndex={0} aria-label={"Desktop remoto "+profile.name}
      onPointerDown={e=>pointer(e,"down")} onPointerUp={e=>pointer(e,"up")}
      onPointerMove={e=>pointer(e,"move")} onContextMenu={e=>e.preventDefault()}
      onWheel={wheel} onKeyDown={e=>key(e,true)} onKeyUp={e=>key(e,false)}
      onBlur={releaseKeys}/>
    {!hasFrame&&<div className="rdp-placeholder">Aguardando primeiro frame RDP...</div>}
  </div>
  <div className="rdp-footer" role="status">
    <span>{progress}</span>
    <span className="muted">Clique no desktop para usar teclado e mouse. PoC: sem IME, clipboard ou redimensionamento remoto.</span>
  </div>
</section>;
}
function App(){const[profiles,setProfiles]=useState<Profile[]>([]);const[form,setForm]=useState({...empty,id:""});const[tabs,setTabs]=useState<Tab[]>([]);const[active,setActive]=useState("");const[error,setError]=useState("");const[search,setSearch]=useState("");
async function refresh(){try{setProfiles(await invoke<Profile[]>("profiles_list"))}catch(e){setError(String(e))}}
useEffect(()=>{refresh()},[]);
async function save(e:React.FormEvent){e.preventDefault();try{await invoke("profiles_save",{input:{...form,id:form.id||null,port:Number(form.port)}});setForm({...empty,id:""});await refresh()}catch(e){setError(String(e))}}
async function open(p:Profile){setError("");if(p.protocol==="rdp"){const id=crypto.randomUUID();setTabs(prev=>[...prev,{id,profile:p,rdpStatus:"Verificando conexão TCP..."}]);setActive(id);invoke<{address:string;security:string;note:string}>("rdp_negotiate",{profileId:p.id}).then(result=>setTabs(prev=>prev.map(t=>t.id===id?{...t,rdpStatus:result.security+" — "+result.note}:t))).catch(e=>setTabs(prev=>prev.map(t=>t.id===id?{...t,rdpStatus:String(e)}:t)));return}try{const sid=await invoke<string>("ssh_open",{profileId:p.id});const id=crypto.randomUUID();setTabs(prev=>[...prev,{id,profile:p,sid}]);setActive(id)}catch(e){setError(String(e))}}
async function close(t:Tab){if(t.sid)await invoke("ssh_close",{id:t.sid}).catch(console.error);setTabs(prev=>prev.filter(x=>x.id!==t.id));if(active===t.id)setActive("")}
return <div className="app"><aside className="sidebar"><h2>RemoteHub <small className="muted">0.0.4-dev</small></h2><p className="muted">SSH validado · RDP gráfico experimental</p><input placeholder="Buscar..." value={search} onChange={e=>setSearch(e.target.value)}/><h3>Conexões</h3>{profiles.filter(p=>(p.name+" "+p.host+" "+p.folder).toLowerCase().includes(search.toLowerCase())).map(p=><div key={p.id}><button className="item" onClick={()=>open(p)}>{p.protocol.toUpperCase()} · {p.name}<span className="muted"> ({p.host})</span></button><div className="row"><button onClick={()=>setForm({...p})}>Editar</button><button onClick={async()=>{if(confirm("Excluir perfil?")){await invoke("profiles_delete",{id:p.id});refresh()}}}>Excluir</button></div></div>)}<h3>{form.id?"Editar":"Nova"} conexão</h3><form className="form" onSubmit={save}><input required placeholder="Nome" value={form.name} onChange={e=>setForm({...form,name:e.target.value})}/><select value={form.protocol} onChange={e=>setForm({...form,protocol:e.target.value,port:e.target.value==="rdp"?3389:22})}><option value="ssh">SSH</option><option value="rdp">RDP (IronRDP experimental)</option></select><input required placeholder="Host" value={form.host} onChange={e=>setForm({...form,host:e.target.value})}/><input required type="number" min="1" max="65535" value={form.port} onChange={e=>setForm({...form,port:Number(e.target.value)})}/><input placeholder="Usuário" value={form.username} onChange={e=>setForm({...form,username:e.target.value})}/><input placeholder="Pasta" value={form.folder} onChange={e=>setForm({...form,folder:e.target.value})}/><button type="submit">Salvar perfil</button></form></aside><main className="work"><div className="tabs">{tabs.map(t=><button className={"tab "+(active===t.id?"selected":"")} key={t.id} onClick={()=>setActive(t.id)}>{t.profile.name} <span onClick={e=>{e.stopPropagation();close(t)}}> ×</span></button>)}</div>{error&&<div className="banner">{error} <button onClick={()=>setError("")}>×</button></div>}{tabs.length===0&&<div className="empty">Selecione SSH para conectar ou RDP para testar acessibilidade TCP.</div>}{tabs.map(t=><div key={t.id} style={{display:active===t.id?"flex":"none",flex:1,minHeight:0}}>{t.sid?<Shell sid={t.sid}/>:<RdpPanel profile={t.profile} status={t.rdpStatus} visible={active===t.id}/>}</div>)}</main></div>}
createRoot(document.getElementById("root")!).render(<App/>);
