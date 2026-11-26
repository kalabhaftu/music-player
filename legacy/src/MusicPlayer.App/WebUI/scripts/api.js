const pending = new Map();
const listeners = new Set();
let nextId = 0;

function receive(event) {
  const message = event.data;
  if (!message || typeof message !== "object") return;
  if (message.type === "result" && pending.has(message.id)) {
    const { resolve, reject, timer, command } = pending.get(message.id);
    clearTimeout(timer);
    pending.delete(message.id);
    if (message.ok) resolve(message.data);
    else {
      const error=new Error(message.error || "The command could not be completed.");
      error.musicPlayerCommand=command;
      reject(error);
    }
    return;
  }
  if (message.type === "event") for (const listener of listeners) listener(message.name, message.data);
}

if (window.chrome?.webview) window.chrome.webview.addEventListener("message", receive);
function safeErrorName(error) {
  const name=typeof error?.name==="string"?error.name:"Error";
  return /^[A-Za-z][A-Za-z0-9]{0,39}$/.test(name)?name:"Error";
}
function safeScriptName(filename) {
  try {
    const uri=new URL(filename,location.href);
    if(uri.origin!==location.origin)return "";
    const leaf=uri.pathname.split("/").pop()||"";
    return /^[A-Za-z0-9._-]{1,80}$/.test(leaf)?leaf:"";
  } catch { return ""; }
}
window.addEventListener("error", event => {
  void command("reportFrontendError",{kind:"script",name:safeErrorName(event.error),source:safeScriptName(event.filename),line:Number(event.lineno)||0,column:Number(event.colno)||0}).catch(()=>{});
});
window.addEventListener("unhandledrejection", event => {
  const reason=event.reason;
  void command("reportFrontendError",{kind:"rejection",name:safeErrorName(reason),command:typeof reason?.musicPlayerCommand==="string"?reason.musicPlayerCommand:""}).catch(()=>{});
});

export function command(name, payload = {}) {
  if (!window.chrome?.webview) return Promise.reject(new Error("The native Music Player connection is unavailable."));
  const id = `web-${++nextId}`;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); const error=new Error("The request timed out.");error.musicPlayerCommand=name;reject(error); }, name === "searchLyrics" ? 110000 : 30000);
    pending.set(id, { resolve, reject, timer, command:name });
    window.chrome.webview.postMessage({ type: "command", id, name, payload });
  });
}

export function onEvent(listener) { listeners.add(listener); return () => listeners.delete(listener); }
