// One themed, keyboard-accessible dropdown for every native select in the app.
const controls=new WeakMap();
let active=null,serial=0,typeBuffer="",typeTimer;
export function syncSelect(select){
  const control=controls.get(select);if(!control)return;
  control.button.textContent=select.selectedOptions[0]?.textContent||"Choose…";
  control.button.disabled=select.disabled;
}
function close(restoreFocus=false){
  if(!active)return;
  const {button,popup}=active;active=null;popup.remove();button.setAttribute("aria-expanded","false");button.removeAttribute("aria-activedescendant");
  if(restoreFocus&&button.isConnected)button.focus();
}
function focusOption(index,direction=1){
  if(!active)return;
  const options=[...active.popup.children];if(!options.length)return;
  index=Math.max(0,Math.min(options.length-1,index));
  while(index>=0&&index<options.length&&active.select.options[index].disabled)index+=direction;
  if(index<0||index>=options.length)return;
  active.index=index;
  options.forEach((option,i)=>option.classList.toggle("highlighted",i===active.index));
  active.button.setAttribute("aria-activedescendant",options[active.index].id);
  options[active.index].scrollIntoView({block:"nearest"});
}
function choose(index){
  if(!active)return;
  const {select}=active,option=select.options[index];if(!option||option.disabled)return;
  select.selectedIndex=index;syncSelect(select);close(true);
  select.dispatchEvent(new Event("change",{bubbles:true}));
}
function open(select,button){
  if(active?.select===select){close();return;}
  close();
  const popup=document.createElement("div"),id=`select-list-${++serial}`;
  popup.id=id;popup.className="select-popup";popup.setAttribute("role","listbox");popup.setAttribute("aria-label",button.getAttribute("aria-label"));
  [...select.options].forEach((option,index)=>{
    const row=document.createElement("div");row.id=`${id}-${index}`;row.className="select-option";row.textContent=option.textContent;row.setAttribute("role","option");row.setAttribute("aria-selected",String(index===select.selectedIndex));row.setAttribute("aria-disabled",String(option.disabled));
    row.addEventListener("pointerdown",event=>event.preventDefault());row.addEventListener("click",()=>choose(index));row.addEventListener("pointermove",()=>focusOption(index));popup.append(row);
  });
  document.body.append(popup);button.setAttribute("aria-controls",id);button.setAttribute("aria-expanded","true");
  const bounds=button.getBoundingClientRect(),width=Math.min(Math.max(bounds.width,220),window.innerWidth-24);
  popup.style.width=`${width}px`;popup.style.left=`${Math.max(12,Math.min(bounds.left,window.innerWidth-width-12))}px`;
  const below=window.innerHeight-bounds.bottom-12,above=bounds.top-12;
  popup.style.maxHeight=`${Math.max(60,Math.min(320,Math.max(below,above)))}px`;
  if(below>=Math.min(240,above))popup.style.top=`${bounds.bottom+6}px`;else popup.style.bottom=`${window.innerHeight-bounds.top+6}px`;
  active={select,button,popup,index:select.selectedIndex};focusOption(Math.max(0,select.selectedIndex));
}
function enhance(select){
  if(controls.has(select))return;
  const wrapper=document.createElement("span"),button=document.createElement("button");wrapper.className="select-control";button.type="button";button.className="select select-trigger";
  button.setAttribute("role","combobox");button.setAttribute("aria-haspopup","listbox");button.setAttribute("aria-expanded","false");
  button.setAttribute("aria-label",select.getAttribute("aria-label")||select.closest(".setting-row")?.querySelector("b")?.textContent||({sortSelect:"Sort tracks",duplicateSort:"Sort duplicates",eqPreset:"Equalizer preset",outputDevice:"Output device",crossfade:"Crossfade"})[select.id]||select.id||"Choose an option");
  select.before(wrapper);wrapper.append(select,button);select.classList.add("select-native");select.hidden=true;select.tabIndex=-1;select.setAttribute("aria-hidden","true");controls.set(select,{button});syncSelect(select);
  button.addEventListener("click",()=>open(select,button));select.addEventListener("change",()=>syncSelect(select));
  button.addEventListener("keydown",event=>{
    if(["ArrowDown","ArrowUp","Home","End","Enter"," ","Escape"].includes(event.key)){
      event.preventDefault();if(event.key==="Escape"){close(true);return;}
      if(!active||active.select!==select){open(select,button);if(event.key==="Enter"||event.key===" ")return;}
      if(event.key==="Enter"||event.key===" ")choose(active.index);
      else focusOption(event.key==="Home"?0:event.key==="End"?select.options.length-1:active.index+(event.key==="ArrowUp"?-1:1),["ArrowUp","End"].includes(event.key)?-1:1);
    }else if(event.key==="Tab")close();
    else if(event.key.length===1&&!event.ctrlKey&&!event.metaKey){
      event.preventDefault();if(!active||active.select!==select)open(select,button);
      typeBuffer+=event.key.toLocaleLowerCase();clearTimeout(typeTimer);typeTimer=setTimeout(()=>typeBuffer="",600);
      const index=[...select.options].findIndex(option=>!option.disabled&&option.textContent.toLocaleLowerCase().startsWith(typeBuffer));if(index>=0)focusOption(index);
    }
  });
  new MutationObserver(()=>syncSelect(select)).observe(select,{childList:true,subtree:true,attributes:true,attributeFilter:["disabled","selected"],characterData:true});
}
export function installSelects(){
  const discover=root=>{if(root.matches?.("select:not(.select-native)"))enhance(root);root.querySelectorAll?.("select:not(.select-native)").forEach(enhance);};
  new MutationObserver(records=>{if(active&&!active.button.isConnected)close();for(const record of records)for(const node of record.addedNodes)if(node.nodeType===1)discover(node);}).observe(document.body,{childList:true,subtree:true});discover(document);
  document.addEventListener("pointerdown",event=>{if(active&&!active.popup.contains(event.target)&&event.target!==active.button)close();});
  document.addEventListener("keydown",event=>{if(event.key==="Escape"&&active){event.preventDefault();event.stopImmediatePropagation();close(true);}},true);
  document.addEventListener("scroll",event=>{if(active&&!active.popup.contains(event.target))close();},true);
  window.addEventListener("resize",()=>close());
}
