import { command, onEvent } from "./api.js";
import { createLibraryViews } from "./library.js";
import { createPlayerUi } from "./player.js";
import { lyricLinesMarkup, parseLyricsText, createLyricsSearch, createLyricsLoader } from "./lyrics.js";
import { contrastingInk } from "./colors.js";
import { installSelects, syncSelect } from "./selects.js";

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
const icons = {
  home:"M3 10.8 12 3l9 7.8v9.7a.5.5 0 0 1-.5.5h-6v-6h-5v6h-6a.5.5 0 0 1-.5-.5z",
  search:"M20 20l-4.5-4.5M18 10.5a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0Z",
  clock:"M12 7v5l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z",
  heart:"M20.8 8.7c0 4.1-8.8 10-8.8 10s-8.8-5.9-8.8-10A4.7 4.7 0 0 1 12 6.4a4.7 4.7 0 0 1 8.8 2.3Z",
  music:"M9 18V5l12-2v13M9 18c0 1.4-1.4 2.5-3 2.5S3 19.4 3 18s1.4-2.5 3-2.5 3 1.1 3 2.5Zm12-2c0 1.4-1.4 2.5-3 2.5s-3-1.1-3-2.5 1.4-2.5 3-2.5 3 1.1 3 2.5Z",
  disc:"M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm0 6a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm0 3h.01",
  users:"M16 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2m16 0v-2a4 4 0 0 0-3-3.87M14 3.13a4 4 0 0 1 0 7.75M10 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z",
  tag:"M20.6 13.6 13.7 20.5a2 2 0 0 1-2.8 0L3.5 13.1V3.5h9.6l7.5 7.4a2 2 0 0 1 0 2.7ZM7.5 7.5h.01",
  trending:"M3 17l6-6 4 4 8-8M15 7h6v6",
  film:"M4 4h16v16H4zM4 9h16M9 4v5m6-5v5M9 9v11m6-11v11",
  spark:"m12 3 1.7 5.3L19 10l-5.3 1.7L12 17l-1.7-5.3L5 10l5.3-1.7L12 3Zm7 12 .8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8L19 15Z",
  list:"M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01",
  folder:"M3 6.5A1.5 1.5 0 0 1 4.5 5H10l2 2h7.5A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z",
  headphones:"M3 13v-1a9 9 0 0 1 18 0v1m-18 0v5a2 2 0 0 0 2 2h2v-8H5a2 2 0 0 0-2 1Zm18 0v5a2 2 0 0 1-2 2h-2v-8h2a2 2 0 0 1 2 1Z",
  lyrics:"M4 5h16M4 10h16M4 15h10M4 20h12",
  queue:"M4 6h12M4 11h12M4 16h7m4-2v7l6-3.5z",
  sliders:"M4 21v-7m0-4V3m8 18v-9m0-4V3m8 18v-5m0-4V3M2 14h4m4-6h4m4 6h4",
  copy:"M8 8V4h12v12h-4M4 8h12v12H4z",
  settings:"M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Zm0-5v2m0 14v2m9-9h-2M5 12H3m15.4-6.4-1.4 1.4M7 17l-1.4 1.4m12.8 0L17 17M7 7 5.6 5.6",
  equalizer:"M4 18V6m5 15V3m5 18V8m5 13V5M2 11h4m1-3h4m1 6h4m1-5h4",
  more:"M5 12h.01M12 12h.01M19 12h.01",
  "panel-left":"M4 4h16v16H4zM9 4v16",
  "chevron-left":"m15 18-6-6 6-6", "chevron-right":"m9 18 6-6-6-6", close:"M18 6 6 18M6 6l12 12",
  refresh:"M20 7v5h-5M4 17v-5h5m-4-3a8 8 0 0 1 13.6-3L20 7M4 17l1.4 1.4A8 8 0 0 0 19 15",
  shuffle:"m18 14 4 4-4 4m0-20 4 4-4 4M2 18h2.5a5 5 0 0 0 4-2l7-8a5 5 0 0 1 4-2H22M2 6h2.5a5 5 0 0 1 4 2l1.5 1.7m4 4.6 1.5 1.7a5 5 0 0 0 4 2H22",
  "skip-back":"M19 20 9 12l10-8v16ZM5 19V5", "skip-forward":"m5 4 10 8-10 8V4Zm14 1v14",
  play:"m7 4 14 8-14 8V4Z", pause:"M7 5h3v14H7zm7 0h3v14h-3z", repeat:"m17 2 4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4m14-1v2a3 3 0 0 1-3 3H3",
  expand:"M3 9V4a1 1 0 0 1 1-1h5M15 3h5a1 1 0 0 1 1 1v5M21 15v5a1 1 0 0 1-1 1h-5M9 21H4a1 1 0 0 1-1-1v-5",
  minimize:"M8 3v5H3m13-5v5h5M8 21v-5H3m13 5v-5h5",
  speaker:"M3 9v6h4l5 4V5L7 9H3Zm13-2a7 7 0 0 1 0 10m2-13a11 11 0 0 1 0 16", volume:"M3 9v6h4l5 4V5L7 9H3Zm13-2a7 7 0 0 1 0 10", mute:"M3 9v6h4l5 4V5L7 9H3Zm13 1 5 5m0-5-5 5"
};

function svg(name) { return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${icons[name] || icons.music}"/></svg>`; }
function paintIcons(root = document) { $$('[data-icon]', root).forEach(node => { const name = node.dataset.icon; node.innerHTML = svg(name); }); }
function esc(value) { return String(value ?? "").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c])); }
function initials(value) { return String(value || "MP").split(/\s+/).filter(Boolean).slice(0,2).map(word => word[0]).join("").toUpperCase() || "MP"; }
function cover(item, extra = "") {
  const seed = String(item?.album ? `${item.albumArtist||""}:${item.album}` : item?.artist || item?.name || item?.title || item?.id || "music");
  let hash = 2166136261; for (const ch of seed) hash = Math.imul(hash ^ ch.charCodeAt(0),16777619);
  const cls = `c${Math.abs(hash >>> 0) % 10 + 1}`;
  const artwork = item?.artworkUrl ? `<img loading="lazy" src="${esc(item.artworkUrl)}" alt="" onerror="this.onerror=null;this.hidden=true;if(this.nextElementSibling)this.nextElementSibling.hidden=false">` : "";
  const mark=initials(item?.album || item?.artist || item?.name || item?.title);
  return `<div class="cover ${cls} ${extra}">${artwork}<div class="cover-art" ${artwork?"hidden":""}>${esc(mark)}</div></div>`;
}
function displayText(value) {
  const text=String(value??"").trim();
  return text&&!/^(?:undefined|null)$/i.test(text)?text:"";
}
function titleOf(item) { return displayText(item?.name)||displayText(item?.title)||"Unknown"; }
function subOf(item) { return displayText(item?.artist)||displayText(item?.albumArtist)||displayText(item?.meta)||""; }
function fmtDuration(seconds) { const n = Math.max(0, Math.floor(Number(seconds) || 0)); return n >= 3600 ? `${Math.floor(n/3600)}:${String(Math.floor(n/60)%60).padStart(2,"0")}:${String(n%60).padStart(2,"0")}` : `${Math.floor(n/60)}:${String(n%60).padStart(2,"0")}`; }
function bytesLabel(bytes) { const value = Number(bytes)||0; return value > 1024**3 ? `${(value/1024**3).toFixed(1)} GB` : value > 1024**2 ? `${(value/1024**2).toFixed(0)} MB` : `${(value/1024).toFixed(0)} KB`; }

const state = { view:"Home", search:"", filter:"all", searchQuery:"", searchPages:{songs:0,albums:0,artists:0,playlists:0}, searchResults:{}, duplicateSort:"Title", duplicateDescending:false, duplicateOffset:0, sort:"Title", descending:false, videoSupportEnabled:false, offset:0, pageSize:200, total:0, items:[], group:null, playlist:null, track:null, trackDetails:null, playing:false, position:0, duration:0, volume:75, shuffle:false, repeat:"Off", repeatA:null, repeatB:null, queue:[], queueTotal:0, queueOffset:0, queueIndex:-1, panel:"queue", panelOpen:true, history:["Home"], historyIndex:0, settings:{}, modal:null, muted:false, scan:null, loading:false, lyricLines:[], lyricText:"", lyricsRaw:"", lyricsSource:"none", lyricsLoadingTrackId:"", lyricsAutoError:"", lyricsRevision:0, lyricsLoadRevision:0, immersiveLyricsOpen:false, lyricsSearchResults:[], lyricsSearchTrackId:"", updateCheckActive:false, aboutInfo:null };
const lyricsSearch = createLyricsSearch(trackId => call("searchLyrics", {id:trackId}, false));
const automaticLyrics = createLyricsLoader(trackId=>call("getLyrics",{id:trackId}),trackId=>searchLyricsResults(trackId,false));
async function searchLyricsResults(trackId, manual=false) {
  return lyricsSearch.search(trackId,{manual});
}
async function ensureLyricsLoaded(trackId, allowRemote=!!state.settings.autoLoadLyrics) {
  return automaticLyrics.load(trackId,{allowRemote,track:state.track?.id===trackId?state.track:null});
}
async function loadLyricsForTrack(trackId,allowRemote=!!state.settings.autoLoadLyrics,force=false) {
  if(!force&&trackId&&state.lyricsLoadingTrackId===trackId)return null;
  if(!force&&trackId&&state.lyricsTrack?.id===trackId&&(state.lyricLines.length||!allowRemote||state.lyricsSource==="lrclib"))return null;
  if(!trackId){state.lyricsLoadRevision++;state.lyricsLoadingTrackId="";state.lyricsTrack=null;state.lyricLines=[];state.lyricText="";state.lyricsRaw="";state.lyricsSource="none";state.lyricsRevision++;updatePlayer();return null;}
  const revision=++state.lyricsLoadRevision;
  state.lyricsLoadingTrackId=trackId;state.lyricsAutoError="";
  updatePlayer();
  if(state.view==="Lyrics")void renderView();
  try {
    const data=await ensureLyricsLoaded(trackId,allowRemote);
    if(revision!==state.lyricsLoadRevision||(state.track?.id!==trackId&&state.trackId!==trackId))return data;
    state.lyricsTrack=data.track||state.track||null;state.lyricLines=data.lines||[];state.lyricText=data.plainText||"";state.lyricsRaw=data.raw||"";state.lyricsSource=data.source||"none";state.lyricsAutoError=data.autoLookupMessage||(data.autoLookupFailed?"Automatic lookup did not find a confident match. You can search LRCLIB manually.":"");state.lyricsLoadingTrackId="";state.lyricsRevision++;
    updatePlayer();
    if(state.view==="Lyrics")void renderView();
    return data;
  } catch {
    if(revision===state.lyricsLoadRevision){state.lyricsLoadingTrackId="";state.lyricsAutoError="Lyrics could not be loaded. You can try Search LRCLIB manually.";updatePlayer();}
    return null;
  }
}
const layoutDrag={sidebarWidth:null,rightWidth:null,sidebarCollapsed:null};
let panelOpenSave=Promise.resolve();
function clamp(value,min,max){return Math.max(min,Math.min(max,Number(value)||min))}
function navigationViewOrder(settings=state.settings){
  const views=$$("#navigation .nav-item[data-view]").map(item=>item.dataset.view);
  const stored=Array.isArray(settings?.navigationOrder)?settings.navigationOrder:[];
  const custom=[...new Set(stored.filter(view=>view!=="Home"&&views.includes(view)))];
  return ["Home",...custom,...views.filter(view=>view!=="Home"&&!custom.includes(view))];
}
async function persistNavigationOrder(order){
  state.settings.navigationOrder=order.filter(view=>view!=="Home");
  applyNavigationSettings(state.settings);
  const list=$(".nav-settings");if(list)list.innerHTML=navigationSettingsMarkup(state.settings);
  try{await call("updateSettings",{settings:{navigationOrder:state.settings.navigationOrder}});}
  catch{toast("The sidebar order changed, but could not be saved.");}
}
const settingsAliases={appearance:["theme","color","colour"],artwork:["cover","album art"],audio:["sound","equalizer","eq"],library:["folder","scan","index"],lyrics:["lyric","lrc","timed text","captions"],navigation:["sidebar","menu","reorder"],playback:["song","player"],transparency:["transparent","glass"],updates:["update","release"]};
function normalizeSettingWords(value){return String(value||"").normalize("NFD").replace(/[\u0300-\u036f]/g,"").toLowerCase().replace(/[^a-z0-9]+/g," ").trim().split(/\s+/).filter(word=>word.length>1&&!new Set(["the","and","for","with","from","how","can","does","make","show","use","set","get"]).has(word));}
function editDistanceAtMostOne(a,b){if(Math.abs(a.length-b.length)>1)return false;let i=0,j=0,edits=0;while(i<a.length&&j<b.length){if(a[i]===b[j]){i++;j++;continue;}if(++edits>1)return false;if(a.length>b.length)i++;else if(b.length>a.length)j++;else{i++;j++;}}return edits+(i<a.length||j<b.length?1:0)<=1;}
function settingsMatch(query,text){
  const wanted=normalizeSettingWords(query),words=normalizeSettingWords(text);if(!wanted.length)return true;
  const matches= wanted.filter(term=>{
    const related=[term,...Object.entries(settingsAliases).filter(([,aliases])=>aliases.includes(term)).map(([key])=>key),...(settingsAliases[term]||[])];
    return related.some(candidate=>words.some(word=>word.startsWith(candidate)||candidate.startsWith(word)||(candidate.length>=5&&word.length>=5&&editDistanceAtMostOne(candidate,word))));
  }).length;
  return matches/ wanted.length>=.75;
}
function highlightSettingText(element,query){
  const original=element.dataset.searchOriginalText??element.textContent??"";element.dataset.searchOriginalText=original;
  const terms=[...new Set(normalizeSettingWords(query).flatMap(word=>[word,...Object.entries(settingsAliases).filter(([,aliases])=>aliases.some(alias=>normalizeSettingWords(alias).includes(word))).map(([key])=>key),...(settingsAliases[word]||[]).flatMap(normalizeSettingWords)]))].sort((a,b)=>b.length-a.length).map(word=>word.replace(/[.*+?^${}()|[\]\\]/g,"\\$&"));
  if(!terms.length){element.textContent=original;return;}
  const pattern=new RegExp(`(${terms.join("|")})`,"ig"),parts=original.split(pattern);element.replaceChildren(...parts.map(part=>{if(terms.some(term=>part.toLowerCase()===term.toLowerCase())){const mark=document.createElement("mark");mark.className="settings-search-highlight";mark.textContent=part;return mark;}return document.createTextNode(part);}));
}
function applySettingsSearch(query=$("#globalSearch")?.value||""){
  const grid=$(".settings-grid"),panel=$(".settings-panel");if(!grid||!panel)return;
  const active=!!normalizeSettingWords(query).length;grid.classList.toggle("settings-searching",active);
  let matches=0;
  for(const section of $$(".settings-panel .setting-section")){
    let sectionMatches=0;
    for(const row of $$(".setting-row",section)){
      const original=row.dataset.searchOriginalText??row.textContent??"";row.dataset.searchOriginalText=original;
      const match=settingsMatch(query,original);
      row.hidden=active&&!match;
      if(match){matches++;sectionMatches++;}
      for(const label of $$(':scope > div:first-child > b,:scope > div:first-child > span',row))highlightSettingText(label,query);
    }
    section.hidden=active&&!sectionMatches;
  }
  let empty=$("#settingsSearchEmpty",panel);
  if(active&&matches===0&&!empty){empty=document.createElement("p");empty.id="settingsSearchEmpty";empty.className="settings-search-empty";empty.textContent="No matching settings. Try a broader phrase.";panel.append(empty);}
  if(empty)empty.hidden=!active||matches>0;
}
function prepareSettingsSearch(){
  const panel=$(".settings-panel");if(!panel)return;
  const save=$( '[data-action="save-settings"]',panel);save?.closest(".toolbar")?.remove();
  const navSettings=$(".nav-settings",panel),navHint=navSettings?.previousElementSibling;if(navHint?.tagName==="P")navHint.textContent="Drag the grip or use the arrows to reorder. Use the switch to hide a destination.";
  applySettingsSearch($("#globalSearch")?.value||"");
}
const globalSearch=$("#globalSearch"),librarySearchPlaceholder=globalSearch.placeholder;
function updateGlobalSearchMode(){
  const settingsMode=state.view==="Settings",mode=settingsMode?"settings":"library";
  globalSearch.placeholder=settingsMode?"Search settings…":librarySearchPlaceholder;
  globalSearch.setAttribute("aria-label",settingsMode?"Search settings":"Search songs, artists, albums, and playlists");
  if(globalSearch.dataset.searchMode!==mode){
    globalSearch.dataset.searchMode=mode;globalSearch.value="";state.search="";
    ++searchInputRevision;clearTimeout(searchTimer);
  }
  if(settingsMode)prepareSettingsSearch();
}
new MutationObserver(updateGlobalSearchMode).observe($("#routeView"),{childList:true});
const settingPreviousValues=new WeakMap();
async function persistSettingControl(control,value){
  const key=control?.dataset.setting;if(!key)return false;
  const previous=settingPreviousValues.has(control)?settingPreviousValues.get(control):state.settings[key];settingPreviousValues.delete(control);
  try{await call("updateSettings",{settings:{[key]:value}});state.settings={...state.settings,[key]:value};setTheme(state.settings);return true;}
  catch(error){state.settings={...state.settings,[key]:previous};if(control.matches(".toggle")){control.classList.toggle("on",!!previous);control.setAttribute("aria-checked",String(!!previous));}else if(control.value!==undefined)control.value=previous??"";setTheme(state.settings);toast(error?.message||"This setting could not be saved.");return false;}
}
$("#routeView").addEventListener("input",event=>{const control=event.target;if(control.matches("[data-setting]:not(.toggle)")&&!settingPreviousValues.has(control))settingPreviousValues.set(control,state.settings[control.dataset.setting]);},true);
$("#routeView").addEventListener("change",async event=>{const control=event.target;if(!control.matches("[data-setting]:not(.toggle)"))return;event.preventDefault();event.stopImmediatePropagation();await persistSettingControl(control,control.value);if(control.dataset.setting==="windowTransparency"){const output=control.parentElement?.querySelector("output");if(output)output.value=control.value+"%";}},true);
$("#routeView").addEventListener("click",async event=>{const control=event.target.closest("[data-setting].toggle");if(!control)return;event.preventDefault();event.stopImmediatePropagation();const key=control.dataset.setting,wasEnabled=control.classList.contains("on"),enabled=!wasEnabled;control.classList.toggle("on",enabled);control.setAttribute("aria-checked",String(enabled));if(await persistSettingControl(control,enabled)){if(key==="autoLoadLyrics"&&!wasEnabled&&enabled&&state.track)void loadLyricsForTrack(state.track.id,true);if(key==="transparentWindow"){const range=$("[data-setting='windowTransparency']");if(range)range.disabled=!enabled;}}},true);
function applyLayoutPreferences(){
  const collapsed=window.innerWidth<=840||(layoutDrag.sidebarCollapsed??state.settings.sidebarCollapsed??false);
  const maxSidebar=Math.max(180,Math.min(360,window.innerWidth-360-(window.innerWidth>900&&state.panelOpen?220:0)));
  const expandedWidth=clamp(layoutDrag.sidebarWidth??state.settings.navigationWidth??232,180,maxSidebar);
  const sidebarWidth=collapsed?78:expandedWidth;
  const maxRight=Math.max(240,window.innerWidth-sidebarWidth-360);
  const rightWidth=clamp(layoutDrag.rightWidth??state.settings.rightPanelWidth??294,240,Math.min(460,maxRight));
  document.documentElement.style.setProperty("--sidebar",`${sidebarWidth}px`);
  document.documentElement.style.setProperty("--right",`${rightWidth}px`);
  const root=$("#appRoot");
  const columnWidths=state.settings.songColumnWidths||{};
  for(const key of ["title","album","added","year","plays","duration"]){const width=Number(columnWidths[key]);if(Number.isFinite(width)&&width>0)root?.style.setProperty(`--song-col-${key}`,`${width}px`);else root?.style.removeProperty(`--song-col-${key}`);}
  root?.classList.toggle("sidebar-collapsed",collapsed);
  const compactPanel=window.innerWidth<=1000||window.innerWidth-sidebarWidth-rightWidth<420;
  if(!compactPanel||!root.classList.contains("panel-compact"))state.panelCompactExpanded=false;
  const panelVisible=compactPanel?!!state.panelCompactExpanded:!!state.panelOpen;
  root.classList.toggle("panel-open",!!state.panelOpen);
  root.classList.toggle("panel-closed",!state.panelOpen);
  root.classList.toggle("panel-compact",compactPanel);
  root.classList.toggle("panel-peek",compactPanel&&panelVisible);
  const panelScroll=$("#panelScroll");panelScroll.inert=!panelVisible;panelScroll.setAttribute("aria-hidden",String(!panelVisible));
  $("#panelExpand").setAttribute("aria-expanded",String(panelVisible));
  const panelToggle=$("#panelToggleBtn"),panelLabel=panelVisible?"Collapse queue sidebar":"Expand queue sidebar";
  panelToggle.setAttribute("aria-label",panelLabel);panelToggle.title=panelLabel;panelToggle.setAttribute("aria-expanded",String(panelVisible));
  if(!panelVisible&&$("#rightPanel").contains(document.activeElement)&&document.activeElement!==$("#panelExpand"))
    (compactPanel?$("#panelExpand"):panelToggle).focus();
  const toggle=$("#sidebarToggleBtn");
  if(toggle){const label=collapsed?"Expand sidebar":"Collapse sidebar";toggle.title=label;toggle.setAttribute("aria-label",label);toggle.setAttribute("aria-pressed",String(collapsed));}
  const navResize=$("#sidebarResize"); if(navResize)navResize.setAttribute("aria-valuenow",String(Math.round(sidebarWidth)));
  const panelResize=$("#panelResize"); if(panelResize)panelResize.setAttribute("aria-valuenow",String(Math.round(rightWidth)));
}
let scanOutcomeGeneration=0;
const playerUi = createPlayerUi({state,$,$$,call,command,cover,esc,fmtDuration,svg,lyricLinesMarkup,loadLyricsForTrack}); const {updatePlayer,updateActiveLyric,updatePanel,loadTrackDetails,refreshCurrent,setVolume,toggleMute}=playerUi;
let toastTimer;
let searchTimer;
let searchInputRevision=0;
updateGlobalSearchMode();

async function call(name,payload={},notifyError=true) { try { return await command(name,payload); } catch (error) { if(notifyError)toast(error.message || "Something went wrong."); throw error; } }
function toast(message) { const box=$("#toast"); box.textContent=message; box.classList.add("show"); clearTimeout(toastTimer); toastTimer=setTimeout(()=>box.classList.remove("show"),2800); }
function applyUpdateCheckState(checking=state.updateCheckActive){state.updateCheckActive=!!checking;const button=$('[data-action="check-updates"]');if(!button)return;button.disabled=state.updateCheckActive;button.setAttribute("aria-busy",String(state.updateCheckActive));button.textContent=state.updateCheckActive?"Checking…":"Check for updates";}
function setTheme(settings) {
  const theme=String(settings?.theme||"System").toLowerCase();
  const resolvedTheme=theme==="light"||theme==="system"&&settings?.resolvedTheme==="Light"?"light":"dark";
  document.documentElement.dataset.theme=resolvedTheme;
  const root=document.documentElement;
  const transparency=settings?.transparentWindow?clamp(settings.windowTransparency??0,0,60):0;
  root.dataset.transparent=String(!!settings?.transparentWindow);
  root.style.setProperty("--window-transparency",`${transparency}%`);
  const palette=settings?.accentMode==="Artwork"?settings?.artworkPalette:null;
  root.dataset.artwork=String(!!palette);
  if(palette){
    const colors={"--art-background":palette.background,"--art-sidebar":palette.sidebar,"--art-main":palette.main,"--art-panel":palette.panel,"--art-raised":palette.raised,"--art-hover":palette.hover,"--art-control":palette.control};
    for(const [key,value] of Object.entries(colors))if(/^#[0-9a-f]{6}$/i.test(String(value||"")))root.style.setProperty(key,value);
  }
  document.documentElement.dataset.motion=String(settings?.motionStyle||"Subtle").toLowerCase();
  const nativeAccent=resolvedTheme==="light"?"#548c00":"#b7ff2d";
  const color=settings?.accentManual&&settings?.accentColor?settings.accentColor:settings?.accentMode==="Artwork"&&settings?.artworkAccent?settings.artworkAccent:nativeAccent;
  if(/^#[0-9a-f]{6}$/i.test(color)){
    document.documentElement.style.setProperty("--accent",color);
    const r=parseInt(color.slice(1,3),16),g=parseInt(color.slice(3,5),16),b=parseInt(color.slice(5,7),16);
    document.documentElement.style.setProperty("--accent-rgb",`${r},${g},${b}`);
    document.documentElement.style.setProperty("--accent-ink",contrastingInk(color));
  }
  const selectionColor=settings?.selectionColorMode==="Custom"&&/^#[0-9a-f]{6}$/i.test(String(settings?.selectionColor||""))?settings.selectionColor:color;
  if(/^#[0-9a-f]{6}$/i.test(selectionColor)){
    root.style.setProperty("--selection-color",selectionColor);
    root.style.setProperty("--selection-ink",contrastingInk(selectionColor));
  }
  const selectionColorControl=$('[data-setting="selectionColor"]');
  if(selectionColorControl)selectionColorControl.disabled=settings?.selectionColorMode!=="Custom";
  if(settings?.browseWidth) document.documentElement.style.setProperty("--browse-width",`${Number(settings.browseWidth)}px`);
  const transparencySlider=$('[data-setting="windowTransparency"]');
  if(transparencySlider){transparencySlider.disabled=!settings?.transparentWindow;const output=transparencySlider.parentElement?.querySelector("output");if(output)output.value=`${transparency}%`;}
  applyNavigationSettings(settings);
  applyLayoutPreferences();
}

function applyNavigationSettings(settings=state.settings) {
  const items=$$("#navigation .nav-item[data-view]"); const order=navigationViewOrder(settings);
  const hidden=new Set(Array.isArray(settings?.hiddenPanels)?settings.hiddenPanels:[]);hidden.delete("Home");
  // `order` only accepts integers in CSS. Leave room before each navigation
  // item for its section heading so saved ordering can cross section groups.
  for(const item of items){ item.hidden=hidden.has(item.dataset.view); item.style.order=String(Math.max(0,order.indexOf(item.dataset.view))*2+1); }
  const visibleItems=items.filter(item=>!item.hidden).sort((a,b)=>Number(a.style.order)-Number(b.style.order));
  const visiblePosition=new Map(visibleItems.map((item,index)=>[item,index]));
  const sections=$$("#navigation .nav-section"); sections.forEach(section=>{
    const heading=section.querySelector(".nav-title"); if(!heading)return;
    const sectionItems=$$(".nav-item[data-view]",section).filter(item=>!item.hidden);
    const positions=sectionItems.map(item=>visiblePosition.get(item)).filter(Number.isInteger);
    const firstOrder=Math.min(...sectionItems.map(item=>Number(item.style.order)));
    const contiguous=positions.length>0&&Math.max(...positions)-Math.min(...positions)+1===positions.length;
    heading.hidden=!contiguous;
    if(contiguous) heading.style.order=String(Math.max(0,firstOrder-1));
  });
}

const libraryViews = createLibraryViews({state,$,$$,svg,paintIcons,esc,initials,cover,titleOf,subOf,fmtDuration,bytesLabel,call,toast,setTheme,applyNavigationSettings,openModal,updatePlayer,updatePanel,ensureLyricsLoaded,lyricLinesMarkup});
const {trackRow,songTable,card,cardGrid,section,viewHeader,toolbar,navigate,renderView,loadMore,loadArtistAlbums,renderHome,renderSearch,viewFilter,renderTracksView,renderGroupsView,renderFolders,renderPlaylists,renderPlaylistDetail,renderGroupDetail,renderQueue,renderNowPlaying,renderLyrics,renderAudio,renderSettings,navigationSettingsMarkup,renderDuplicates,openDuplicateFiles}=libraryViews;
function openModal(title, body, actions) { delete $("#modalLayer").dataset.lyricsSearchToken; state.modal = { title, body, actions }; $("#modalTitle").textContent = title; $("#modalBody").innerHTML = body; $("#modalActions").innerHTML = actions || `<button class="action" data-modal-close>Close</button>`; $("#modalLayer").hidden = false; paintIcons($("#modalLayer")); $$("#modalBody input,#modalBody textarea")[0]?.focus(); }
function closeModal() { delete $("#modalLayer").dataset.lyricsSearchToken; $("#modalLayer").hidden = true; state.modal = null; }
function askConfirm(title,message,confirmLabel,action) { openModal(title,`<p>${esc(message)}</p>`,`<button class="action" data-modal-close>Cancel</button><button class="action ${confirmLabel.toLowerCase().includes("delete")?"danger":"primary"}" data-confirm="${esc(action)}">${esc(confirmLabel)}</button>`); }
function openUiResetDialog(){
  const choices=[["appearance","Colors and appearance","Theme, artwork palette, selection color, transparency, and motion"],["layout","Panels and navigation","Sidebar and Now Playing widths, collapse state, navigation order, and browse spacing"],["libraryDisplay","Library display","Visible song columns, resized column widths, and duplicate handling"]];
  const body=`<p>Choose which interface preferences to restore. Every group is selected by default.</p>${choices.map(([key,title,description])=>`<label class="reset-choice"><input type="checkbox" data-reset-category="${key}" checked><span><b>${title}</b><small>${description}</small></span></label>`).join("")}`;
  openModal("Reset UI settings",body,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="reset-ui-settings">Reset selected settings</button>`);
}
function contextMenu(x,y,id) { const menu=$("#contextMenu"); menu.dataset.id=id; menu.innerHTML=`<button data-menu-action="play">Play</button><button data-menu-action="play-next">Play next</button><button data-menu-action="queue">Add to queue</button><button data-menu-action="playlist">Add to playlist</button><button data-menu-action="create-playlist-with-track">Create playlist with this song…</button><button data-menu-action="favorite">Toggle favorite</button><button data-menu-action="rating">Set rating…</button><hr><button data-menu-action="lyrics">Lyrics</button><button data-menu-action="tags">Edit tags</button><button data-menu-action="restore-tags">Restore previous tags</button><button data-menu-action="details">Track details</button><button data-menu-action="location">Show in File Explorer</button>`; menu.classList.add("show");const bounds=menu.getBoundingClientRect();menu.style.left=`${Math.max(8,Math.min(x,innerWidth-bounds.width-8))}px`;menu.style.top=`${Math.max(8,Math.min(y,innerHeight-bounds.height-8))}px`; }

async function addToPlaylist(trackId) { const data=await call("getPlaylists"); if(!data.playlists?.length) return newPlaylist(trackId); openModal("Add to playlist",`<label class="modal-field"><span>Choose a playlist</span><select class="field" id="playlistChoice">${data.playlists.map(p=>`<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("")}</select></label>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="add-to-playlist" data-track-id="${esc(trackId)}">Add</button>`); }
function newPlaylist(trackId="") { openModal("New playlist",`<label class="modal-field"><span>Playlist name</span><input class="field" id="playlistName" maxlength="120" placeholder="Playlist name"></label>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="create-playlist" data-track-id="${esc(trackId)}">Create playlist</button>`); }
async function openDetails(id) {
  const [d,tags]=await Promise.all([call("getTrackDetails",{id}),call("getTags",{id})]);const t=d.track;
  const fields=[["Title",t.title],["Artist",t.artist],["Album",t.album],["Album artist",t.albumArtist],["Genre",t.genre],["Year",t.year],["Track number",t.trackNumber],["Format",d.container],["Duration",fmtDuration(d.durationSeconds)],["Bitrate",d.bitrateKbps?`${d.bitrateKbps} kbps`:"—"],["Sample rate",d.sampleRateHz?`${d.sampleRateHz} Hz`:"—"],["Bits per sample",d.bitsPerSample||"—"],["File size",bytesLabel(d.fileSize)],["Modified",d.modifiedDisplay],["Location",d.path]];
  const additional=Object.entries(tags.additionalFields||{}).filter(([,value])=>String(value||"").trim()).map(([key,value])=>[key.replaceAll("_"," "),value]);
  const custom=Object.entries(tags.customFields||{}).map(([key,value])=>[key,value]);
  const backups=(tags.backups||[]).map(item=>item.created);
  const detailList=items=>items.length?`<dl class="details">${items.map(([key,value])=>`<div><dt>${esc(key)}</dt><dd>${esc(value||"—")}</dd></div>`).join("")}</dl>`:`<p class="muted">None recorded.</p>`;
  openModal("Track details",`${detailList(fields)}<h3>Additional standard tags</h3>${detailList(additional)}<h3>Custom tags</h3>${detailList(custom)}<h3>Recent tag backups</h3>${backups.length?`<ul>${backups.map(created=>`<li>${esc(created)}</li>`).join("")}</ul>`:`<p class="muted">No tag backups.</p>`}`);
}

async function editLyrics(initialText = null,trackId=state.lyricsTrack?.id||state.trackId||state.track?.id) { const d=await call("getLyrics",{id:trackId});const source=initialText!==null?"LRCLIB result (not saved)":({sidecar:"Sidecar file",embedded:"Embedded audio tags",none:"No saved lyrics"})[d.source]||"Unknown source";openModal("Edit lyrics",`<p>${esc(d.track?.title||"")} · ${esc(d.track?.artist||"")}</p><p class="lyric-source">Current lyrics: ${esc(source)}</p><div class="toolbar"><button class="action" id="stampLyric">Timestamp at current position</button><label>Timing offset (ms) <input class="field" id="lyricsOffset" type="number" value="${Number(d.offsetMilliseconds)||0}"></label><select class="select" id="lyricsMode"><option value="embed" selected>Embed in audio file</option><option value="sidecar">Save beside audio file</option></select></div><textarea class="textarea" id="lyricsEditor">${esc(initialText??d.raw??"")}</textarea>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="save-lyrics" data-track-id="${esc(d.track?.id||"")}">Save lyrics</button>`); $("#stampLyric").addEventListener("click",()=>{const editor=$("#lyricsEditor"),time=Math.max(0,state.position),m=Math.floor(time/60),s=(time%60).toFixed(2).padStart(5,"0"),stamp=`[${m}:${s}]`;const p=editor.selectionStart;editor.setRangeText(`${stamp}`,p,p,"end");editor.focus();}); }
async function editTags(id) { const d=await call("getTags",{id}); const t=d.track; const fields=[["title","Title"],["artist","Artist"],["album","Album"],["albumArtist","Album artist"],["genre","Genre"],["year","Year"],["trackNumber","Track number"]]; const custom=Object.entries(d.customFields||{}).map(([key,value])=>`<label class="modal-field"><span>${esc(key)}</span><input class="field" data-custom-tag="${esc(key)}" value="${esc(value)}"></label>`).join("");const additional=Object.entries(d.additionalFields||{}).map(([key,value])=>`<label class="modal-field"><span>${esc(key.replaceAll("_"," "))}</span><input class="field" data-additional-tag="${esc(key)}" value="${esc(value)}"></label>`).join(""); openModal("Edit tags",`${fields.map(([key,label])=>`<label class="modal-field"><span>${label}</span><input class="field" data-tag="${key}" value="${esc(t[key]||"")}"></label>`).join("")}<div class="toolbar"><button class="action" id="chooseArtwork">Choose artwork…</button><span id="artworkPath"></span></div>${custom?`<details><summary>Additional format-specific fields</summary>${custom}</details>`:""}${additional?`<details><summary>Additional standard fields</summary>${additional}</details>`:""}<p>${esc(d.customFormat||"Audio tags")} · Saving creates a recoverable backup.</p>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="save-tags" data-track-id="${esc(id)}">Save tags</button>`); $("#modalLayer").dataset.artworkToken="";$("#chooseArtwork").addEventListener("click",async()=>{const picked=await call("pickArtwork");if(picked?.token){$("#modalLayer").dataset.artworkToken=picked.token;$("#artworkPath").textContent=picked.name;}}); }

async function action(name,el) {
  const id=el?.dataset.id||el?.dataset.trackId||""; const view=state.view;
  const trackContext={view,search:state.search,sort:state.sort,descending:state.descending,group:state.group,playlistId:state.playlist?.id};
  switch(name) {
    case "play": case "play-track": await call("playTrack",{id,...trackContext}); await refreshCurrent(); break;
    case "play-view": await call("playView",{view:state.view,group:state.group}); await refreshCurrent(); break;
    case "shuffle-view": await call("shuffleView",{view:state.view,group:state.group}); await refreshCurrent(); break;
    case "play-group": await call("playGroup",{type:el.dataset.type,id}); await refreshCurrent(); break;
    case "play-playlist-card": await call("playPlaylist",{playlistId:id}); await refreshCurrent(); break;
    case "favorite": await call("toggleFavorite",{id:id||state.track?.id}); break;
    case "context": contextMenu(el.getBoundingClientRect().right,el.getBoundingClientRect().bottom,id); break;
    case "play-next": await call("playNext",{id}); await refreshCurrent(); break;
    case "queue": await call("addToQueue",{id}); await refreshCurrent(); break;
    case "playlist": await addToPlaylist(id); break;
    case "rating": openModal("Set rating",`<label class="modal-field"><span>Rating</span><select class="field" id="ratingValue"><option value="0">Unrated</option>${[1,2,3,4,5].map(n=>`<option value="${n}">${"★".repeat(n)}</option>`).join("")}</select></label>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="set-rating" data-track-id="${esc(id)}">Save</button>`); break;
    case "tags": await editTags(id); break;
    case "restore-tags": {const d=await call("getTags",{id});if(!d.backups?.length){toast("No saved tag backups are available.");break;}const choice=d.backups.length>1?`<label class="modal-field"><span>Backup version</span><select class="field" id="backupChoice">${d.backups.map(b=>`<option value="${esc(b.id)}">${esc(b.created)}</option>`).join("")}</select></label>`:"";openModal("Restore previous tags?",`<p>The selected version replaces the current tags. The current file will also be backed up.</p>${choice}`,`<button class="action" data-modal-close>Cancel</button><button class="action danger" data-confirm="restore-tags">Restore tags</button>`);$("#modalLayer").dataset.trackId=id;if(d.backups.length===1)$("#modalLayer").dataset.backupId=d.backups[0].id;break;}
    case "lyrics": state.trackId=id; await navigate("Lyrics"); break;
    case "details": await openDetails(id||state.track?.id); break;
    case "location": case "show-location": await call("showInFolder",{id}); break;
    case "new-playlist": newPlaylist(); break;
    case "import-playlist": await call("importPlaylist"); await renderView(); break;
    case "create-playlist-with-track": newPlaylist(id); break;
    case "rename-playlist": openModal("Rename playlist",`<label class="modal-field"><span>Playlist name</span><input class="field" id="playlistName" maxlength="120" value="${esc(state.playlist.name)}"></label>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="rename-playlist">Save</button>`); break;
    case "delete-playlist": askConfirm("Delete playlist?",`Delete “${state.playlist.name}”? The music files will not be changed.`,"Delete playlist","delete-playlist"); break;
    case "play-playlist": await call("playPlaylist",{playlistId:state.playlist.id}); await refreshCurrent(); break;
    case "shuffle-playlist": await call("shufflePlaylist",{playlistId:state.playlist.id}); await refreshCurrent(); break;
    case "export-playlist": await call("exportPlaylist",{playlistId:state.playlist.id}); break;
    case "remove-playlist-track": await call("removePlaylistTrack",{playlistId:state.playlist.id,position:Number(el.dataset.position)}); await renderView(); break;
    case "add-folder": await call("addFolder"); await renderView(); break;
    case "add-exclusion": await call("addExclusion"); await manageExclusions(); break;
    case "remove-exclusion": await call("removeExclusion",{path:el.dataset.path}); await manageExclusions(); break;
    case "toggle-extension": await call("setExtensionEnabled",{extension:el.dataset.extension,enabled:el.dataset.enabled!=="true",kind:el.dataset.kind}); await manageExclusions(); break;
    case "add-extension": openModal("Add a file type",`<label class="modal-field"><span>One extension</span><input class="field" id="extensionValue" maxlength="13" placeholder=".mkv" autocomplete="off" spellcheck="false"></label><p>Enter one extension, such as <b>.mkv</b> or <b>mp4</b>. You can add more one at a time. Common formats can be enabled from the list in Scan exclusions.</p><label class="modal-field"><span>Media type</span><select class="field" id="extensionKind"><option>Video</option><option>Audio</option></select></label><p>Music Player will try the bundled media engine; playback still depends on codec support.</p>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-action="add-extension-confirm">Add and enable</button>`); break;
    case "add-extension-confirm": {const extension=$("#extensionValue")?.value||"",kind=$("#extensionKind")?.value||"Video";await call("setExtensionEnabled",{extension,enabled:true,kind});closeModal();await manageExclusions();break;}
    case "show-file": await call("showFilePath",{path:el.dataset.path}); break;
    case "copy-location": await call("copyTrackPath",{id:el.dataset.id}); toast("File path copied."); break;
    case "manage-roots": await manageRoots(); break;
    case "manage-exclusions": await manageExclusions(); break;
    case "remove-root": askConfirm("Remove folder?",`Stop scanning ${el.dataset.path} and remove its unshared tracks from the index? Music files and playlist entries remain.`,"Remove folder","remove-root"); $("#modalLayer").dataset.path=el.dataset.path; break;
    case "scan": await call("scanLibrary"); break;
    case "rebuild-index": await call("rebuildLibraryIndex"); break;
    case "duplicates": await navigate("Duplicates"); break;
    case "scan-pause": await call("toggleScanPause"); break;
    case "scan-cancel": await call("cancelScan"); break;
    case "clear-queue": askConfirm("Clear upcoming queue?","Remove upcoming tracks from the queue? The current track keeps playing.","Clear queue","clear-queue"); break;
    case "play-queue": await call("playQueueEntry", { index: Number(el.dataset.index) }); break;
    case "queue-remove": await call("removeQueue", { index: Number(el.dataset.index) }); break;
    case "edit-lyrics": await editLyrics(); break;
    case "search-lyrics": {
      if (el?.dataset.busy === "true") break;
      const trackId=state.lyricsTrack?.id||state.trackId||state.track?.id;
      if (!trackId) { toast("Choose a track before searching for lyrics."); break; }
      const token = `${Date.now()}-${Math.random()}`;
      state.lyricsSearchResults = [];
      state.lyricsSearchTrackId=trackId||"";
      state.lyricsSearchModalToken = token;
      if (el) { el.dataset.busy = "true"; el.disabled = true; el.setAttribute("aria-busy", "true"); el.textContent = "Searching…"; }
      openModal("Find lyrics", `<div class="lyric-search-state" aria-live="polite"><span class="lyric-search-spinner" aria-hidden="true"></span><div><b>Searching for lyrics…</b><p>Searching LRCLIB, an online lyrics catalog. This may take a few seconds.</p></div></div>`);
      $("#modalLayer").dataset.lyricsSearchToken = token;
      try {
        const results = await searchLyricsResults(trackId, true);
        if (state.lyricsSearchModalToken !== token || $("#modalLayer").dataset.lyricsSearchToken !== token) break;
        state.lyricsSearchResults = results;
      const body=results.length?`<div class="lyric-search-results">${results.map((r,i)=>{
        const hasLyrics=!!String(r.syncedLyrics||r.plainLyrics||"").trim();
        return `<button type="button" class="action lyric-search-result" data-lyric-result="${i}" ${hasLyrics?"":"disabled aria-disabled=\"true\""}><span>${esc(r.trackName||"Untitled")}</span><small>${esc(r.artistName||"Unknown artist")}${r.albumName?` · ${esc(r.albumName)}`:""}${hasLyrics?parseLyricsText(r.syncedLyrics).lines.length?" · Timed":" · Untimed":""}${hasLyrics?"":" · Lyrics not provided"}</small></button>`;
        }).join("")}</div>` : `<div class="lyric-search-state"><div><b>No matching lyrics found</b><p>Try again in a minute if you expect this track to have lyrics.</p></div></div>`;
        $("#modalTitle").textContent = "Find lyrics";
        $("#modalBody").innerHTML = body;
      } catch (error) {
        if (state.lyricsSearchModalToken === token && $("#modalLayer").dataset.lyricsSearchToken === token)
          $("#modalBody").innerHTML = `<div class="lyric-search-state is-error"><div><b>Could not find lyrics</b><p>${esc(error?.message || "Check your internet connection and try again shortly.")}</p></div></div>`;
      } finally {
        if (el) { delete el.dataset.busy; el.disabled = false; el.removeAttribute("aria-busy"); el.textContent = "Find lyrics"; }
      }
      break;
    }
    case "refresh-devices": await refreshAudioControls(await call("refreshAudioDevices")); break;
    case "save-eq": openModal("Save equalizer preset",`<label class="modal-field"><span>Preset name</span><input class="field" id="presetName" maxlength="60"></label>`,`<button class="action" data-modal-close>Cancel</button><button class="action primary" data-modal-command="save-eq">Save preset</button>`); break;
    case "check-updates": { if(el.dataset.busy==="true")break;el.dataset.busy="true";applyUpdateCheckState(true);try{await call("checkUpdates");}catch{}finally{el.dataset.busy="false";applyUpdateCheckState(false);}break;}
    case "open-release": await call("openRelease",{url:el.dataset.url}); break;
    case "open-legal": await call("openLegalFile",{name:el.dataset.file}); break;
    case "open-default-apps": await call("openDefaultApps"); break;
    case "open-logs": await call("openLogs"); break;
    case "export-logs": await call("exportLogs"); break;
    case "toggle-mute": await toggleMute(); break;
    case "reset-ui": openUiResetDialog(); break;
    case "load-more": await loadMore(el); break;
    case "load-artist-albums": await loadArtistAlbums(el); break;
    case "load-folders": await loadMore(el); break;
    case "direction": state.descending=!state.descending; state.offset=0; await renderView(); break;
    case "search-page": {const kind=el.dataset.type;state.searchPages[kind]=Math.max(0,(state.searchPages[kind]||0)+Number(el.dataset.direction||0));await renderView();break;}
    case "retry-search": state.searchQuery="";state.searchResults={};await renderView();break;
    case "duplicate-page": state.duplicateOffset=Math.max(0,state.duplicateOffset+Number(el.dataset.direction||0)*100); await renderView(); break;
    case "duplicate-direction": state.duplicateDescending=!state.duplicateDescending; state.duplicateOffset=0; await renderView(); break;
    case "duplicate-files": await openDuplicateFiles(id); break;
    case "duplicate-files-page": await openDuplicateFiles(id,Math.max(0,Number(el.dataset.offset)||0)); break;
    default: break;
  }
}

async function manageRoots() { const d=await call("getFolders",{rootsOnly:true}); openModal("Library folders",(d.roots||[]).map(r=>{const available=r.available!==false&&r.status!=="Unavailable";return `<div class="setting-row ${available?"":"folder-unavailable"}"><div><b>${esc(r.name||r.path)} · ${available?"Available":"Unavailable"}</b><span>${esc(r.path)} · ${Number(r.trackCount||0)} tracks${available?"":" · Reconnect this drive, then rescan the library."}</span></div><button class="action" data-action="remove-root" data-path="${esc(r.path)}">Remove</button></div>`;}).join("")+`<div class="toolbar modal-action-row"><button class="action" data-action="add-folder">Add folder…</button><button class="action" data-action="scan">Rescan library</button></div>`); }
async function manageExclusions() {
  const d=await call("getExclusions");
  const previousVideoSupport=state.videoSupportEnabled;
  state.videoSupportEnabled=(d.extensions||[]).some(item=>item.kind==="Video"&&item.enabled);
  if(!state.videoSupportEnabled){if(state.view==="Videos")await navigate("Playlists",false);if(state.sort==="Type")state.sort="Title";}
  if(previousVideoSupport!==state.videoSupportEnabled&&["Songs","Playlists","Videos"].includes(state.view))await renderView();
  const types=(d.extensions||[]).map(item=>`<div class="setting-row extension-setting"><div><b>${esc(item.extension)}</b><span>${esc(item.kind)} file${item.enabled?" · included in scans":" · excluded from scans"}</span></div><button class="toggle ${item.enabled?"on":""}" data-action="toggle-extension" data-extension="${esc(item.extension)}" data-kind="${esc(item.kind)}" data-enabled="${item.enabled}" role="switch" aria-checked="${item.enabled}"><i></i></button></div>`).join("");
  const paths=(d.paths||[]).map(p=>`<div class="setting-row"><span>${esc(p)}</span><button class="action" data-action="remove-exclusion" data-path="${esc(p)}">Remove</button></div>`).join("");
  openModal("Scan exclusions",`<h3>File extensions</h3><p>Audio types are included by default. Video types are off until enabled. Enabled video files open in a separate Music Player window and use the media engine already included with the app; playback still depends on the file’s codec.</p>${types}<div class="toolbar modal-action-row"><button class="action" data-action="add-extension">Add a supported file type…</button></div><h3>Folders</h3><p>Windows and common app/cache locations are skipped automatically. Excluding a folder also excludes its subfolders.</p>${paths}<div class="toolbar modal-action-row"><button class="action" data-action="add-exclusion">Add ignored folder…</button></div>`);
}
async function toggleAudioView(){
  if(state.view!=="Audio"){await navigate("Audio");return;}
  let previousIndex=-1;
  for(let index=Math.min(state.historyIndex-1,state.history.length-1);index>=0;index--)if(state.history[index]!=="Audio"){previousIndex=index;break;}
  if(previousIndex<0){state.history=["Home"];state.historyIndex=0;await navigate("Home",false);return;}
  state.historyIndex=previousIndex;
  await navigate(state.history[previousIndex],false);
}
async function seekToLyricLine(element){
  if(state.settings.seekFromLyrics===false)return;
  if(!state.track||state.lyricsTrack?.id!==state.track.id)throw new Error("Play this track before seeking from its lyrics.");
  const seconds=Number(element?.dataset.lyricSeconds);
  if(!Number.isFinite(seconds)||seconds<0)return;
  await call("seek",{seconds});state.position=seconds;state.positionUpdatedAt=performance.now();state.seeking=false;state.seekPreviewSeconds=null;updatePlayer();
}
async function handleMenu(actionName,id) { $("#contextMenu").classList.remove("show"); const actions={play:"play-track","play-next":"play-next",queue:"queue",playlist:"playlist","create-playlist-with-track":"create-playlist-with-track",favorite:"favorite",rating:"rating",lyrics:"lyrics",tags:"tags","restore-tags":"restore-tags",details:"details",location:"location"}; await action(actions[actionName],{dataset:{id}}); }

document.addEventListener("click",async e=>{
  const target=e.target instanceof Element?e.target:null;
  const lyricResult=target?.closest("[data-lyric-result]");
  if(lyricResult){
    e.preventDefault();
    if(lyricResult.disabled)return;
    try{
      const index=Number(lyricResult.dataset.lyricResult);
      const selected=state.lyricsSearchResults[index];
      const trackId=state.lyricsSearchTrackId||state.lyricsTrack?.id||state.trackId||state.track?.id;
      if(!selected)throw new Error("That LRCLIB result is no longer available. Search again and choose a result.");
      const synced=String(selected.syncedLyrics||"").trim();
      const lyrics=parseLyricsText(synced).lines.length?synced:String(selected.plainLyrics||"").trim()||synced;
      if(!lyrics){toast("This LRCLIB result does not include readable lyrics.");return;}
      if(!trackId)throw new Error("Choose a track before opening a lyrics result.");
      lyricResult.disabled=true;
      openModal("Opening lyrics",`<p>Loading the selected lyrics into the editor…</p>`);
      await editLyrics(lyrics,trackId);
    }catch(error){
      openModal("Could not open lyrics",`<p>${esc(error.message||"The selected lyrics could not be opened.")}</p>`);
    }
    return;
  }
  const lyricLine=target?.closest(".lyric-line[data-lyric-seconds]");
  if(lyricLine){e.preventDefault();try{await seekToLyricLine(lyricLine);}catch(error){toast(error.message||"Could not seek to that lyric line.");}return;}
  const queueRow = target?.closest(".queue-table tr[data-queue-index]");
  if (queueRow && !target.closest("button,a,input,[data-action],[data-queue-drag]")) { e.preventDefault(); await action("play-queue", { dataset: { index: queueRow.dataset.queueIndex } }); return; }
  const nav=e.target.closest("[data-view]"); if(nav){ e.preventDefault(); if(nav.dataset.view==="Audio"&&state.view==="Audio")await toggleAudioView();else await navigate(nav.dataset.view||"Home"); return; }
  const menu=e.target.closest("[data-menu-action]"); if(menu){ await handleMenu(menu.dataset.menuAction,$("#contextMenu").dataset.id); return; }
  const filter=e.target.closest("[data-filter]"); if(filter){ state.filter=filter.dataset.filter; await renderView(); return; }
  const open=e.target.closest("[data-open-type]"); if(open && !e.target.closest("[data-action]")){ const type=open.dataset.openType; if(type==="playlist"){state.playlist={id:open.dataset.openId,name:open.dataset.openName};await navigate("Playlist");} else {state.group={column:type,name:open.dataset.openName,id:open.dataset.openId};await navigate(type==="album"?"Album":type==="artist"?"Artist":type==="folder"?"Folder":"Genre");} return; }
  const btn=e.target.closest("[data-action]"); if(btn){ await action(btn.dataset.action,btn); return; }
  const track=e.target.closest("[data-track]"); if(track){ await action("play-track",{dataset:{id:track.dataset.track}}); return; }
  const modalCmd = e.target.closest("[data-modal-command]"); if (modalCmd) {
    const kind = modalCmd.dataset.modalCommand; try {
    if(kind==="create-playlist"){const result=await call("createPlaylist",{name:$("#playlistName").value,trackId:modalCmd.dataset.trackId});closeModal();if(result?.playlist){state.playlist=result.playlist;await navigate("Playlist");}else await renderView();}
    else if(kind==="rename-playlist"){await call("renamePlaylist",{playlistId:state.playlist.id,name:$("#playlistName").value});state.playlist.name=$("#playlistName").value;closeModal();await renderView();}
    else if(kind==="add-to-playlist"){await call("addToPlaylist",{playlistId:$("#playlistChoice").value,trackId:modalCmd.dataset.trackId});closeModal();toast("Added to playlist.");}
    else if(kind==="set-rating"){await call("setRating",{id:modalCmd.dataset.trackId,rating:Number($("#ratingValue").value)});closeModal();await renderView();}
    else if(kind==="save-lyrics"){const trackId=modalCmd.dataset.trackId;modalCmd.disabled=true;try{const result=await call("saveLyrics",{id:trackId,text:$("#lyricsEditor").value,mode:$("#lyricsMode").value,offsetMilliseconds:Number($("#lyricsOffset").value)||0});automaticLyrics.forget(trackId);lyricsSearch.forget(trackId);await loadLyricsForTrack(trackId,false,true);closeModal();toast(result?.storage==="sidecar-fallback"?"The audio file couldn’t be updated, so lyrics were saved beside it.":result?.storage==="sidecar"?"Lyrics saved beside the audio file.":"Lyrics embedded in the audio file.");await renderView();}catch(error){modalCmd.disabled=false;toast(error?.message||"Could not save lyrics. Check file permissions and try saving beside the audio file.");}}
    else if(kind==="save-tags"){const tags={};$$('[data-tag]').forEach(el=>tags[el.dataset.tag]=el.value);tags.customFields={};$$('[data-custom-tag]').forEach(el=>tags.customFields[el.dataset.customTag]=el.value);tags.additionalFields={};$$('[data-additional-tag]').forEach(el=>tags.additionalFields[el.dataset.additionalTag]=el.value);await call("saveTags",{id:modalCmd.dataset.trackId,tags,artworkToken:$("#modalLayer").dataset.artworkToken||""});closeModal();toast("Tags saved with a backup.");await renderView();}
    else if(kind==="save-eq"){await flushEqualizer();await call("saveEqualizerPreset",{name:$("#presetName").value});closeModal();await refreshAudioControls();}
    else if(kind==="reset-ui-settings"){const groups=$$('[data-reset-category]:checked').map(input=>input.dataset.resetCategory);if(!groups.length){closeModal();return;}await call("resetUiSettings",{groups});const bootstrap=await call("getBootstrap");state.settings={...(bootstrap.settings||{}),resolvedTheme:bootstrap.resolvedTheme||"Dark"};state.panel=bootstrap.panel||"queue";closeModal();setTheme(state.settings);applyLayoutPreferences();setPanelOpen(bootstrap.panelOpen??state.settings.rightPanelOpen??(window.innerWidth>1180),false);updatePanel();toast("Selected UI settings restored.");await renderView();}
    } catch (error) { if (kind === "save-lyrics") toast(error?.message || "Could not save lyrics. Check file permissions and try saving beside the audio file."); } return;
  }
  const confirm = e.target.closest("[data-confirm]"); if (confirm) { const kind = confirm.dataset.confirm; const id = $("#modalLayer").dataset.trackId; const path = $("#modalLayer").dataset.path; const backupId = $("#backupChoice")?.value || $("#modalLayer").dataset.backupId; if (kind === "delete-playlist") await call("deletePlaylist", { playlistId: state.playlist.id }); else if (kind === "remove-root") await call("removeRoot", { path }); else if (kind === "clear-queue") await call("clearQueue"); else if (kind === "restore-tags") await call("restoreTags", { id, backupId }); closeModal(); if (kind !== "clear-queue") await renderView(); return; }
  if(e.target.closest("[data-modal-close]")||e.target===$("#modalClose")){closeModal();return;}
  if(!e.target.closest("#contextMenu")) $("#contextMenu").classList.remove("show");
});

document.addEventListener("contextmenu",e=>{const row=e.target.closest("[data-context=track]");if(row){e.preventDefault();contextMenu(e.clientX,e.clientY,row.dataset.contextId||row.dataset.track);}});
let queuePointer = null;
function clearQueueDropTargets() { $$(".queue-table tr.queue-drop-before,.queue-table tr.queue-drop-after").forEach(row => row.classList.remove("queue-drop-before", "queue-drop-after")); }
function clearQueueDrag() {
  const active = queuePointer;
  queuePointer = null;
  document.body.classList.remove("queue-pointer-dragging");
  $$(".queue-table tr.queue-dragging").forEach(row => row.classList.remove("queue-dragging"));
  clearQueueDropTargets();
  if (active?.handle.hasPointerCapture(active.pointerId)) active.handle.releasePointerCapture(active.pointerId);
}
$("#routeView").addEventListener("pointerdown", event => {
  const handle = event.target.closest("[data-queue-drag]");
  if (!handle || handle.disabled || event.button !== 0 || !event.isPrimary || queuePointer) return;
  const index = Number(handle.dataset.queueDrag);
  if (!Number.isInteger(index) || index <= Number(state.queueIndex)) return;
  event.preventDefault();
  queuePointer = { index, handle, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, dragging: false, targetIndex: -1, after: false };
  try { handle.setPointerCapture(event.pointerId); } catch { }
});
$("#routeView").addEventListener("pointermove", event => {
  const active = queuePointer;
  if (!active || active.pointerId !== event.pointerId) return;
  if (!active.dragging && Math.hypot(event.clientX - active.startX, event.clientY - active.startY) < 4) return;
  active.dragging = true;
  event.preventDefault();
  document.body.classList.add("queue-pointer-dragging");
  active.handle.closest("tr[data-queue-index]")?.classList.add("queue-dragging");
  clearQueueDropTargets();
  const row = document.elementFromPoint(event.clientX, event.clientY)?.closest(".queue-table tr[data-queue-index]");
  const targetIndex = Number(row?.dataset.queueIndex);
  if (!row || !Number.isInteger(targetIndex) || targetIndex < Number(state.queueIndex)) { active.targetIndex = -1; return; }
  active.targetIndex = targetIndex;
  active.after = event.clientY >= row.getBoundingClientRect().top + row.offsetHeight / 2;
  if (targetIndex === Number(state.queueIndex) && !active.after) { active.targetIndex = -1; return; }
  row.classList.add(active.after ? "queue-drop-after" : "queue-drop-before");
});
$("#routeView").addEventListener("pointerup", async event => {
  const active = queuePointer;
  if (!active || active.pointerId !== event.pointerId) return;
  event.preventDefault();
  const boundary = active.targetIndex + (active.after ? 1 : 0);
  const toIndex = boundary - (active.index < boundary ? 1 : 0);
  const shouldMove = active.dragging && active.targetIndex >= Number(state.queueIndex) && toIndex !== active.index;
  const fromIndex = active.index;
  clearQueueDrag();
  if (shouldMove) {
    try { await call("moveQueue", { index: fromIndex, toIndex }); }
    catch (error) { toast(error.message || "That queue entry could not be moved."); }
  }
});
$("#routeView").addEventListener("pointercancel", event => { if (queuePointer?.pointerId === event.pointerId) clearQueueDrag(); });
$("#routeView").addEventListener("lostpointercapture", event => { if (queuePointer?.pointerId === event.pointerId) clearQueueDrag(); });
document.addEventListener("keydown",async e=>{
  if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==="k"){e.preventDefault();if(state.view!=="Search"&&state.view!=="Settings")void navigate("Search").catch(error=>toast(error.message||"Search could not be opened."));$("#globalSearch").focus();return;}
  if ((e.key === "Enter" || e.key === " ") && e.target.matches(".queue-table tr[data-queue-index]")) { e.preventDefault(); await action("play-queue", { dataset: { index: e.target.dataset.queueIndex } }); return; }
  if((e.key==="Enter"||e.key===" ")&&e.target.matches(".queue-row[data-action='play-queue']")){e.preventDefault();await action("play-queue",e.target);return;}
  if((e.key==="Enter"||e.key===" ")&&e.target.matches(".location[data-action='copy-location']")){e.preventDefault();await action("copy-location",e.target);return;}
  if(e.code==="Space"&&!e.repeat&&!/INPUT|TEXTAREA|SELECT|BUTTON|A/.test(document.activeElement.tagName)&&!document.activeElement.isContentEditable){e.preventDefault();await call("playPause");await refreshCurrent();return;}
  if((e.altKey||e.metaKey)&&e.key==="ArrowLeft"){e.preventDefault();await call("previous");return;}
  if((e.altKey||e.metaKey)&&e.key==="ArrowRight"){e.preventDefault();await call("next");return;}
  if(e.key==="Enter"&&e.target.matches("tr[data-track]")){await action("play-track",{dataset:{id:e.target.dataset.track}});}
  if(e.key==="Enter"&&e.target.matches("[data-open-type]"))e.target.click();
  if(e.key==="Escape"){if($("#modalLayer").hidden&&!$("#contextMenu").classList.contains("show")&&state.panelCompactExpanded)setCompactPanelExpanded(false);closeModal();$("#contextMenu").classList.remove("show");}
});
document.addEventListener("keydown",async e=>{
  if(e.code!=="Space"||e.repeat||e.ctrlKey||e.altKey||e.metaKey)return;
  const target=e.target instanceof Element?e.target:null;
  const queueEntry=target?.closest(".queue-row[data-action='play-queue']");
  if(queueEntry){e.preventDefault();e.stopImmediatePropagation();try{await action("play-queue",queueEntry);}catch{}return;}
  const location=target?.closest(".location[data-action='copy-location']");
  if(location){e.preventDefault();e.stopImmediatePropagation();try{await action("copy-location",location);}catch{}return;}
  const lyricLine=target?.closest(".lyric-line[data-lyric-seconds]");
  if(lyricLine){e.preventDefault();e.stopImmediatePropagation();try{await seekToLyricLine(lyricLine);}catch{}return;}
  const queueTableRow = target?.closest(".queue-table tr[data-queue-index]");
  if (queueTableRow && target === queueTableRow) { e.preventDefault(); e.stopImmediatePropagation(); try { await action("play-queue", { dataset: { index: queueTableRow.dataset.queueIndex } }); } catch { } return; }
  if (target?.closest("button,a,input,textarea,select,[contenteditable='true'],[role='slider'],[data-lyric-result],#modalLayer [data-modal-close],#modalLayer [data-modal-command]")) return;
  e.preventDefault();e.stopImmediatePropagation();
  try{await call("playPause");await refreshCurrent();}catch{}
},true);

function updateQueueCurrentIndicator() {
  const subtitle=$("#routeView .page-head p");if(state.view==="Queue"&&subtitle)subtitle.textContent=`${Math.max(0,state.queueTotal-state.queueIndex-1)} upcoming tracks`;
  for (const row of $$(".queue-table tbody tr[data-queue-index]")) {
    const index = Number(row.dataset.queueIndex), current = index === Number(state.queueIndex);
    row.classList.toggle("queue-current", current);
    if (current) row.setAttribute("aria-current", "true"); else row.removeAttribute("aria-current");
    const cell = row.querySelector("td.index");
    if (cell) cell.textContent = current ? "▶" : String(index + 1);
    const handle=row.querySelector("[data-queue-drag]");if(handle)handle.disabled=index<=state.queueIndex;
    if (row.hasAttribute("tabindex")) row.setAttribute("aria-label", `Play ${row.querySelector(".song-info b")?.textContent || "track"}${current ? ", currently playing" : ""}`);
  }
}

$("#globalSearch").addEventListener("input",e=>{
  const input=e.currentTarget,query=input.value;
  if(state.view==="Settings"){++searchInputRevision;clearTimeout(searchTimer);applySettingsSearch(query);return;}
  const revision=++searchInputRevision;clearTimeout(searchTimer);searchTimer=setTimeout(async()=>{
    if(revision!==searchInputRevision||query!==input.value)return;
    try { await navigate("Search",state.view!=="Search",query); }
    catch(error) { if(revision===searchInputRevision)toast(error.message||"Search could not be completed."); }
  },220);
});
$("#backBtn").addEventListener("click",()=>{if(state.historyIndex>0){state.historyIndex--;navigate(state.history[state.historyIndex],false);}});
$("#forwardBtn").addEventListener("click",()=>{if(state.historyIndex<state.history.length-1){state.historyIndex++;navigate(state.history[state.historyIndex],false);}});
$("#scanPause").addEventListener("click",()=>call("toggleScanPause")); $("#scanCancel").addEventListener("click",()=>call("cancelScan"));
function showScanDetails(show){$("#scanStrip").hidden=!show;$("#scanToggle").setAttribute("aria-expanded",String(show));}
$("#scanToggle").addEventListener("click",()=>{const widget=$("#scanWidget");widget.classList.remove("details-dismissed");showScanDetails($("#scanStrip").hidden);});
$("#scanWidget").addEventListener("pointerenter",()=>{if(!$("#scanWidget").classList.contains("details-dismissed"))showScanDetails(true);});
$("#scanWidget").addEventListener("pointerleave",()=>{if(!$("#scanWidget").contains(document.activeElement))showScanDetails(false);$("#scanWidget").classList.remove("details-dismissed");});
$("#scanToggle").addEventListener("focus",()=>showScanDetails(true));
$("#scanHide").addEventListener("click",()=>{$("#scanWidget").classList.add("details-dismissed");showScanDetails(false);$("#scanToggle").blur();});
document.addEventListener("pointerdown",event=>{if(!event.target.closest("#scanWidget"))showScanDetails(false);});
$("#topMenuToggle").addEventListener("click",()=>{const menu=$("#topMenu"),show=menu.hidden;menu.hidden=!show;$("#topMenuToggle").setAttribute("aria-expanded",String(show));});
$("#sidebarToggleBtn").addEventListener("click",async()=>{const collapsed=$("#appRoot").classList.contains("sidebar-collapsed");const value=!collapsed;state.settings.sidebarCollapsed=value;layoutDrag.sidebarCollapsed=value;applyLayoutPreferences();try{await call("updateSettings",{settings:{sidebarCollapsed:value}})}catch{}});
$("#topMenu").addEventListener("click",()=>{$("#topMenu").hidden=true;$("#topMenuToggle").setAttribute("aria-expanded","false");});
document.addEventListener("click",e=>{if(!e.target.closest(".top-menu-wrap")){$("#topMenu").hidden=true;$("#topMenuToggle").setAttribute("aria-expanded","false");}});
async function setImmersiveMode(enabled){
  const layer=$("#immersivePlayer");
  if(enabled&&!state.track){toast("Choose a track before opening the full-screen player.");return;}
  if(!enabled)await closeImmersiveDrawer();
  try{await call("setImmersiveMode",{enabled});layer.hidden=!enabled;document.documentElement.classList.toggle("immersive-active",enabled);if(enabled){updatePlayer();revealImmersiveExit();if(state.track)void loadLyricsForTrack(state.track.id,!!state.settings.autoLoadLyrics);}}
  catch{if(enabled){layer.hidden=true;document.documentElement.classList.remove("immersive-active");}}
}
function setImmersiveLyricsOpen(open){
  state.immersiveLyricsOpen=!!open;
  const layer=$("#immersivePlayer"),panel=$("#immersiveLyricsPanel"),toggle=$("#immersiveLyricsToggle");
  layer.classList.toggle("lyrics-open",state.immersiveLyricsOpen);$("#immersiveContent").classList.toggle("lyrics-open",state.immersiveLyricsOpen);
  toggle.setAttribute("aria-expanded",String(state.immersiveLyricsOpen));toggle.setAttribute("aria-label",state.immersiveLyricsOpen?"Hide lyrics":"Show lyrics");toggle.title=state.immersiveLyricsOpen?"Hide lyrics":"Show lyrics";
  $("#immersiveLyricsClose").setAttribute("aria-label","Hide lyrics");$("#immersiveLyricsClose").title="Hide lyrics";
  panel.setAttribute("aria-hidden",String(!state.immersiveLyricsOpen));panel.inert=!state.immersiveLyricsOpen;
  updatePlayer();
  if(state.immersiveLyricsOpen&&state.track)void loadLyricsForTrack(state.track.id,!!state.settings.autoLoadLyrics);
  revealImmersiveExit();
}
let immersiveExitTimer;
let immersiveDrawerReturn=null;
async function closeImmersiveDrawer(){
  if(!immersiveDrawerReturn)return;
  const previous=immersiveDrawerReturn;immersiveDrawerReturn=null;
  $(".main").append($("#content"));$("#immersiveDrawer").hidden=true;
  $$('#immersivePlayer [data-view]').forEach(button=>button.setAttribute("aria-expanded","false"));
  await previous.restore();$("#content").scrollTop=previous.scroll;
  if(state.view==="Queue")void renderView(false,null,true);
  if(state.view==="Audio")void refreshAudioControls();
  if(state.libraryDirty)scheduleLibraryRefresh();
}
async function openImmersiveDrawer(view){
  if(immersiveDrawerReturn&&state.view===view){await closeImmersiveDrawer();return;}
  if(!immersiveDrawerReturn)immersiveDrawerReturn={scroll:$("#content").scrollTop,restore:libraryViews.capturePage()};
  $("#immersiveDrawer").hidden=false;$("#immersiveDrawerBody").append($("#content"));
  $("#immersiveDrawer").dataset.drawerView=view;
  $("#immersiveDrawerTitle").textContent=view==="Audio"?"Equalizer":"Queue";
  $$('#immersivePlayer [data-view]').forEach(button=>button.setAttribute("aria-expanded",String(button.dataset.view===view)));
  await navigate(view,false);$("#immersiveDrawerClose").focus();
}
$("#immersiveDrawerClose").addEventListener("click",()=>closeImmersiveDrawer());
function revealImmersiveExit(){const layer=$("#immersivePlayer");layer.classList.add("controls-visible");clearTimeout(immersiveExitTimer);immersiveExitTimer=setTimeout(()=>layer.classList.remove("controls-visible"),2400);}
$("#playBtn").addEventListener("click",async()=>{await call("playPause");await refreshCurrent();}); $("#prevBtn").addEventListener("click",async()=>{await call("previous");await refreshCurrent();}); $("#nextBtn").addEventListener("click",async()=>{await call("next");await refreshCurrent();});
$("#immersiveToggle").addEventListener("click",()=>setImmersiveMode(true));
$("#immersiveExit").addEventListener("click",()=>setImmersiveMode(false));
$("#immersivePlayer").addEventListener("pointermove",revealImmersiveExit);
$("#immersiveLyricsToggle").addEventListener("click",event=>{event.preventDefault();event.stopPropagation();setImmersiveLyricsOpen(!state.immersiveLyricsOpen);});
$("#immersiveLyricsClose").addEventListener("click",event=>{event.preventDefault();event.stopPropagation();setImmersiveLyricsOpen(false);});
$("#immersivePlayer").addEventListener("click",async event=>{const view=event.target.closest("[data-view]");if(!view)return;event.preventDefault();event.stopPropagation();await openImmersiveDrawer(view.dataset.view);});
document.addEventListener("keydown",event=>{if(event.key==="Escape"&&!$("#immersivePlayer").hidden&&!$(".select-popup")&&$("#modalLayer").hidden){event.preventDefault();if($("#contextMenu").classList.contains("show")){$("#contextMenu").classList.remove("show");event.stopImmediatePropagation();return;}if(immersiveDrawerReturn)void closeImmersiveDrawer();else void setImmersiveMode(false);}},true);
$("#immersivePlay").addEventListener("click",async()=>{await call("playPause");await refreshCurrent();});
$("#immersivePrev").addEventListener("click",async()=>{await call("previous");await refreshCurrent();});
$("#immersiveNext").addEventListener("click",async()=>{await call("next");await refreshCurrent();});
$("#immersiveHeart").addEventListener("click",()=>action("favorite",{dataset:{id:state.track?.id||""}}));
$("#shuffleBtn").addEventListener("click",async()=>{await call("toggleShuffle");await refreshCurrent();}); $("#repeatBtn").addEventListener("click",async()=>{await call("cycleRepeat");await refreshCurrent();}); $("#abBtn").addEventListener("click",async()=>{await call("toggleAbRepeat");await refreshCurrent();}); $("#heartBtn").addEventListener("click",()=>action("favorite",{dataset:{id:state.track?.id||""}}));
const progressRange=$("#progressRange");let lastSeekCommit={seconds:-1,time:0};
function previewSeek(){const seconds=clamp(progressRange.value,0,Math.max(1,state.duration));state.seekPreviewSeconds=seconds;progressRange.style.setProperty("--range-progress",`${seconds/Math.max(1,state.duration)*100}%`);$("#currentTime").textContent=fmtDuration(seconds);}
async function commitSeek(){const seconds=clamp(progressRange.value,0,Math.max(1,state.duration));const now=Date.now();if(lastSeekCommit.seconds===seconds&&now-lastSeekCommit.time<350){state.seeking=false;state.seekPreviewSeconds=null;updatePlayer();return;}lastSeekCommit={seconds,time:now};try{await call("seek",{seconds});state.position=seconds;}finally{state.seeking=false;state.seekPreviewSeconds=null;updatePlayer();}}
progressRange.addEventListener("pointerdown",()=>{state.seeking=true;previewSeek();});
progressRange.addEventListener("input",previewSeek);
progressRange.addEventListener("pointerup",()=>{if(state.seeking)commitSeek().catch(()=>{});});
progressRange.addEventListener("pointercancel",()=>{state.seeking=false;state.seekPreviewSeconds=null;updatePlayer();});
progressRange.addEventListener("change",()=>{if(!state.seeking)state.seeking=true;commitSeek().catch(()=>{});});
progressRange.addEventListener("keydown",e=>{if(["ArrowLeft","ArrowRight","Home","End","PageUp","PageDown"].includes(e.key)){state.seeking=true;}});
progressRange.addEventListener("keyup",e=>{if(state.seeking&&["ArrowLeft","ArrowRight","Home","End","PageUp","PageDown"].includes(e.key))commitSeek().catch(()=>{});});
const immersiveProgress=$("#immersiveProgress");
immersiveProgress.addEventListener("pointerdown",()=>{state.seeking=true;});
immersiveProgress.addEventListener("input",()=>{progressRange.value=immersiveProgress.value;previewSeek();});
immersiveProgress.addEventListener("pointerup",()=>{if(state.seeking){progressRange.value=immersiveProgress.value;commitSeek().catch(()=>{});}});
immersiveProgress.addEventListener("change",()=>{if(!state.seeking)state.seeking=true;progressRange.value=immersiveProgress.value;commitSeek().catch(()=>{});});
$("#volumeRange").addEventListener("input",e=>setVolume(e.target.value)); $("#muteBtn").addEventListener("click",toggleMute);
function setCompactPanelExpanded(open){
  state.panelCompactExpanded=!!open;applyLayoutPreferences();
  if(open)$("#panelClose").focus();
}
function setPanelOpen(open,persist=true){
  if(persist&&$("#appRoot").classList.contains("panel-compact")){setCompactPanelExpanded(open);return;}
  const next=!!open,changed=next!==state.panelOpen;state.panelOpen=next;applyLayoutPreferences();
  if(persist&&changed){state.settings.rightPanelOpen=next;panelOpenSave=panelOpenSave.catch(()=>{}).then(()=>call("updateSettings",{settings:{rightPanelOpen:next}})).catch(()=>{});}
}
function startPanelResize(handle,kind){
  let active=false;
  const move=e=>{if(!active)return;const collapsed=$("#appRoot").classList.contains("sidebar-collapsed"),navWidth=collapsed?78:clamp(layoutDrag.sidebarWidth??state.settings.navigationWidth??232,180,360);if(kind==="sidebar"){const max=Math.max(180,window.innerWidth-360-(state.panelOpen?220:0));layoutDrag.sidebarWidth=clamp(e.clientX,180,Math.min(360,max));layoutDrag.sidebarCollapsed=false;}else{const max=Math.max(240,window.innerWidth-navWidth-360);layoutDrag.rightWidth=clamp(window.innerWidth-e.clientX,240,Math.min(460,max));}applyLayoutPreferences();};
  const finish=async()=>{if(!active)return;active=false;document.documentElement.removeAttribute("data-resizing");const settings={};if(kind==="sidebar"){settings.navigationWidth=String(Math.round(layoutDrag.sidebarWidth??state.settings.navigationWidth??232));settings.sidebarCollapsed=!!layoutDrag.sidebarCollapsed;state.settings={...state.settings,...settings};layoutDrag.sidebarWidth=null;layoutDrag.sidebarCollapsed=null;}else{settings.rightPanelWidth=Number(Math.round(layoutDrag.rightWidth??state.settings.rightPanelWidth??294));state.settings={...state.settings,...settings};layoutDrag.rightWidth=null;}applyLayoutPreferences();try{await call("updateSettings",{settings})}catch{/* The current window keeps the adjusted layout if saving fails. */}};
  handle.addEventListener("pointerdown",e=>{if(e.button!==0)return;e.preventDefault();active=true;document.documentElement.dataset.resizing="true";handle.setPointerCapture(e.pointerId);move(e);});
  handle.addEventListener("pointermove",move);handle.addEventListener("pointerup",finish);handle.addEventListener("pointercancel",finish);handle.addEventListener("lostpointercapture",finish);
  handle.addEventListener("keydown",async e=>{if(!["ArrowLeft","ArrowRight"].includes(e.key))return;e.preventDefault();const delta=(e.shiftKey?20:10)*(e.key==="ArrowRight"?1:-1),settings={};if(kind==="sidebar"){settings.navigationWidth=String(Math.round(clamp((state.settings.navigationWidth??232)+delta,180,360)));settings.sidebarCollapsed=false;}else settings.rightPanelWidth=Number(Math.round(clamp((state.settings.rightPanelWidth??294)-delta,240,460)));state.settings={...state.settings,...settings};applyLayoutPreferences();try{await call("updateSettings",{settings})}catch{}});
}
startPanelResize($("#sidebarResize"),"sidebar");startPanelResize($("#panelResize"),"panel");window.addEventListener("resize",()=>{layoutDrag.sidebarCollapsed=null;applyLayoutPreferences();$("#contextMenu").classList.remove("show");});
function startColumnResize(handle,event){
  if(event.button!==0)return;
  const key=handle.dataset.columnResize,heading=handle.closest("th"),minimum={title:160,album:90,added:76,year:50,plays:50,duration:50}[key]||60;
  if(!heading||!key)return;
  event.preventDefault();event.stopPropagation();
  const startX=event.clientX,startWidth=heading.getBoundingClientRect().width;
  let active=true;
  document.documentElement.dataset.resizing="columns";
  handle.setPointerCapture(event.pointerId);
  const move=e=>{if(!active)return;const width=clamp(Math.round(startWidth+e.clientX-startX),minimum,Math.min(1200,Math.max(minimum,window.innerWidth-80)));state.settings.songColumnWidths={...(state.settings.songColumnWidths||{}),[key]:width};applyLayoutPreferences();};
  const finish=async()=>{if(!active)return;active=false;document.documentElement.removeAttribute("data-resizing");handle.removeEventListener("pointermove",move);handle.removeEventListener("pointerup",finish);handle.removeEventListener("pointercancel",finish);handle.removeEventListener("lostpointercapture",finish);try{await call("updateSettings",{settings:{songColumnWidths:state.settings.songColumnWidths}});}catch{/* Keep the current width visible if settings storage is temporarily unavailable. */}};
  handle.addEventListener("pointermove",move);handle.addEventListener("pointerup",finish);handle.addEventListener("pointercancel",finish);handle.addEventListener("lostpointercapture",finish);
}
document.addEventListener("pointerdown",event=>{const handle=event.target.closest?.("[data-column-resize]");if(handle)startColumnResize(handle,event);});
function updateScanPresentation(scan) {
  state.scan=scan||null;
  const active=!!scan?.active;
  const cancelling=!!scan?.cancelling;
  const terminal=!active&&!!scan?.outcome&&!!scan?.message;
  const phase=cancelling?"Cancelling":scan?.paused?"Paused":active?"Scanning":"";
  const details=`${phase?`${phase} · `:""}${scan?.filesFound||0} tracks · ${scan?.directoriesVisited||0} folders${scan?.currentPath?` · ${scan.currentPath}`:""}`;
  const strip=$("#scanStrip");
  $("#scanWidget").hidden=!active&&!terminal;
  if(!active&&!terminal)showScanDetails(false);
  strip.dataset.outcome=terminal?scan.outcome:"";
  $("#scanWidget").classList.toggle("scan-paused",!!scan?.paused||!active);
  $("#scanWidget").classList.toggle("scan-error",scan?.outcome==="error");
  $("#scanHeading").textContent=active?`${phase} library`:scan?.outcome==="error"?"Scan failed":"Library scan";
  $("#scanToggle").setAttribute("aria-label",active?`${phase} library · ${scan?.filesFound||0} tracks found. Show details`:scan?.message||"Library scan details");
  $("#scanText").textContent=active?details:terminal?scan.message:"";
  $("#scanPause").hidden=!active;
  $("#scanCancel").hidden=!active;
  $("#scanPause").textContent=scan?.paused?"Resume":"Pause";
  $("#scanPause").disabled=cancelling;
  $("#scanCancel").disabled=cancelling;
  const generation=++scanOutcomeGeneration;
  if(terminal)setTimeout(()=>{if(generation===scanOutcomeGeneration){state.scan={...state.scan,outcome:null,message:null};updateScanPresentation(state.scan);}},8000);
  const folderStatus=$("#folderScanStatus");
  if(folderStatus) {
    folderStatus.hidden=!active;
    if(active) {
      $("#folderScanTitle").textContent=cancelling?"Cancelling scan":scan.paused?"Scan paused":"Scanning library";
      $("#folderScanDetails").textContent=details;
    }
  }
  const homeEmpty=$("#homeLibraryEmpty");
  if(homeEmpty) {
    const hasRoots=homeEmpty.dataset.hasRoots==="true";
    $("#homeEmptyTitle").textContent=active?(cancelling?"Cancelling scan":scan.paused?"Scan paused":"Scanning your music folders"):hasRoots?"No indexed tracks yet":"No music folders added";
    $("#homeEmptyMessage").textContent=active?details:hasRoots?"Scan your selected folders to add tracks to the library.":"Add a folder to scan your local collection.";
    const action=$("#homeEmptyAction");
    action.hidden=active;
    action.dataset.action=hasRoots?"scan":"add-folder";
    action.textContent=hasRoots?"Scan folders":"Add music folder";
  }
}
$("#panelClose").addEventListener("click",()=>setPanelOpen(false));$("#panelExpand").addEventListener("click",()=>setCompactPanelExpanded(true));$("#panelToggleBtn").addEventListener("click",()=>{if($("#appRoot").classList.contains("panel-compact"))setCompactPanelExpanded(!state.panelCompactExpanded);else setPanelOpen(!state.panelOpen);});$$('.panel-tabs [data-panel]').forEach(b=>b.addEventListener("click",async()=>{state.panel=b.dataset.panel;updatePanel();await call("setPanelMode",{mode:state.panel});state.settings.rightSidebarMode=state.panel==="info"?"Info":"Queue";}));
async function refreshAudioControls(data=undefined){
  data??=await call("getAudioSettings");
  const editing=!!pendingEqBands||!!eqWrite;const preset=$("#eqPreset");if(preset&&!editing){preset.innerHTML=["Off","Custom",...(data.presets||[])].filter((value,index,all)=>all.indexOf(value)===index).map(name=>`<option ${name==="Custom"?'disabled ':''}value="${esc(name)}">${esc(name)}</option>`).join("");preset.value=data.currentPreset||"Off";syncSelect(preset);}
  if(!editing)for(const input of $$("[data-eq]")){input.value=String(data.bands?.[Number(input.dataset.eq)]?.value||0);input.nextElementSibling.textContent=`${Number(input.value).toFixed(1)} dB`;}
  const output=$("#outputDevice");if(output){output.innerHTML=(data.devices||[]).map(device=>`<option value="${esc(device.id)}">${esc(device.name)}</option>`).join("");output.value=data.selectedId||"";syncSelect(output);}
}
$("#routeView").addEventListener("change",async e=>{if(e.target.id==="sortSelect"){state.sort=e.target.value;state.offset=0;await renderView();}if(e.target.id==="duplicateSort"){state.duplicateSort=e.target.value;state.duplicateOffset=0;await renderView();}if(e.target.id==="outputDevice"){await call("setAudioDevice",{id:e.target.value});await refreshAudioControls();}if(e.target.id==="crossfade")await call("setCrossfade",{seconds:Number(e.target.value)});if(e.target.id==="eqPreset"){await flushEqualizer();await call("setEqualizerPreset",{name:e.target.value});await refreshAudioControls();}});
$("#routeView").addEventListener("click",event=>{const toggle=event.target.closest('[data-setting="transparentWindow"]');if(toggle)requestAnimationFrame(()=>{const range=$('[data-setting="windowTransparency"]');if(range)range.disabled=!toggle.classList.contains("on");});},true);
let pendingEqBands=null,eqWrite=null,eqTimer=null;
function flushEqualizer(){
  clearTimeout(eqTimer);eqTimer=null;if(eqWrite)return eqWrite;
  if(!pendingEqBands)return Promise.resolve();
  eqWrite=(async()=>{try{while(pendingEqBands){const bands=pendingEqBands;pendingEqBands=null;await call("setEqualizerBands",{bands});}}finally{eqWrite=null;}})();return eqWrite;
}
$("#routeView").addEventListener("input",e=>{
  if(e.target.matches("[data-eq]")){
    e.target.nextElementSibling.textContent=`${Number(e.target.value).toFixed(1)} dB`;
    const preset=$("#eqPreset");if(preset&&preset.value!=="Custom"){preset.value="Custom";syncSelect(preset);}
    pendingEqBands=$$("[data-eq]").sort((a,b)=>Number(a.dataset.eq)-Number(b.dataset.eq)).map(input=>Number(input.value));
    if(eqTimer===null)eqTimer=setTimeout(()=>flushEqualizer().catch(error=>toast(error.message||"Equalizer could not be updated.")),50);
  }
  if(e.target.id==="audioVolume")void setVolume(e.target.value);
});
$("#routeView").addEventListener("change",e=>{if(e.target.matches("[data-eq]"))void flushEqualizer().catch(error=>toast(error.message));});
$("#routeView").addEventListener("input",e=>{if(e.target.dataset.setting!=="windowTransparency")return;state.settings.windowTransparency=e.target.value;const output=e.target.parentElement?.querySelector("output");if(output)output.value=e.target.value+"%";setTheme(state.settings);});
let navigationPointer = null;
function clearNavigationDrag() {
  const active = navigationPointer;
  navigationPointer = null;
  document.body.classList.remove("nav-pointer-dragging");
  for (const item of $$(".nav-setting-row")) item.classList.remove("dragging", "drop-before", "drop-after");
  if (active?.handle.hasPointerCapture(active.pointerId)) active.handle.releasePointerCapture(active.pointerId);
}
$("#routeView").addEventListener("pointerdown", event => {
  const handle = event.target.closest("[data-nav-drag-handle]");
  if (!handle || event.button !== 0 || !event.isPrimary || navigationPointer) return;
  const view = handle.dataset.navDragHandle || "";
  if (!view || view === "Home") return;
  event.preventDefault();
  navigationPointer = { view, handle, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, dragging: false, targetView: "", after: false };
  try { handle.setPointerCapture(event.pointerId); } catch { }
});
$("#routeView").addEventListener("pointermove", event => {
  const active = navigationPointer;
  if (!active || active.pointerId !== event.pointerId) return;
  if (!active.dragging && Math.hypot(event.clientX - active.startX, event.clientY - active.startY) < 4) return;
  active.dragging = true;
  event.preventDefault();
  document.body.classList.add("nav-pointer-dragging");
  active.handle.closest(".nav-setting-row")?.classList.add("dragging");
  for (const item of $$(".nav-setting-row")) item.classList.remove("drop-before", "drop-after");
  const row = document.elementFromPoint(event.clientX, event.clientY)?.closest(".nav-setting-row[data-nav-row]");
  if (!row || row.dataset.navRow === active.view) { active.targetView = ""; return; }
  active.targetView = row.dataset.navRow;
  active.after = active.targetView === "Home" || event.clientY >= row.getBoundingClientRect().top + row.offsetHeight / 2;
  row.classList.add(active.after ? "drop-after" : "drop-before");
});
$("#routeView").addEventListener("pointerup", async event => {
  const active = navigationPointer;
  if (!active || active.pointerId !== event.pointerId) return;
  event.preventDefault();
  const order = navigationViewOrder(), from = order.indexOf(active.view), targetIndex = order.indexOf(active.targetView);
  let insert = active.targetView === "Home" ? 1 : targetIndex + (active.after ? 1 : 0);
  clearNavigationDrag();
  if (active.dragging && from >= 1 && targetIndex >= 0 && insert >= 1) {
    if (from < insert) insert--;
    if (from !== insert) { const [moved] = order.splice(from, 1); order.splice(Math.max(1, insert), 0, moved); await persistNavigationOrder(order); }
  }
});
$("#routeView").addEventListener("pointercancel", event => { if (navigationPointer?.pointerId === event.pointerId) clearNavigationDrag(); });
$("#routeView").addEventListener("click",async e=>{const sort=e.target.closest("[data-sort]");if(sort){state.sort=sort.dataset.sort;state.offset=0;await renderView();}const toggle=e.target.closest("[data-setting].toggle");if(toggle){const wasAutoLyrics=!!state.settings.autoLoadLyrics;toggle.classList.toggle("on");toggle.setAttribute("aria-checked",toggle.classList.contains("on"));const s={};s[toggle.dataset.setting]=toggle.classList.contains("on");await call("updateSettings",{settings:s});state.settings={...state.settings,...s};setTheme(state.settings);if(toggle.dataset.setting==="autoLoadLyrics"&&!wasAutoLyrics&&s.autoLoadLyrics&&state.track)void loadLyricsForTrack(state.track.id,true);return;}const move=e.target.closest("[data-nav-move]");if(move){const order=navigationViewOrder(state.settings),index=order.indexOf(move.dataset.navView),next=index+Number(move.dataset.navMove);if(index>0&&next>0&&next<order.length){[order[index],order[next]]=[order[next],order[index]];state.settings.navigationOrder=order.slice(1);applyNavigationSettings(state.settings);$(".nav-settings").innerHTML=navigationSettingsMarkup(state.settings);try{await call("updateSettings",{settings:{navigationOrder:state.settings.navigationOrder}})}catch{}}return;}const visible=e.target.closest("[data-nav-visible]");if(visible){const view=visible.dataset.navVisible;const hidden=new Set(state.settings.hiddenPanels||[]);if(hidden.has(view))hidden.delete(view);else hidden.add(view);state.settings.hiddenPanels=[...hidden];applyNavigationSettings(state.settings);$(".nav-settings").innerHTML=navigationSettingsMarkup(state.settings);try{await call("updateSettings",{settings:{hiddenPanels:state.settings.hiddenPanels}})}catch{}}});
$("#routeView").addEventListener("click",async e=>{
  const tab=e.target.closest("[data-settings-nav]");
  if(tab){$$('[data-settings-nav]').forEach(item=>item.classList.toggle("active",item===tab));const section=document.querySelector('[data-settings-section="'+tab.dataset.settingsNav+'"]');section?.scrollIntoView({behavior:"smooth",block:"start"});return;}
  const dot=e.target.closest("[data-accent-color]");
  if(dot){const settings={accentColor:dot.dataset.accentColor,accentManual:true};state.settings={...state.settings,...settings};setTheme(state.settings);$$(".accent-dot").forEach(item=>item.classList.toggle("active",item===dot));const toggle=$('[data-setting="accentManual"]');if(toggle){toggle.classList.add("on");toggle.setAttribute("aria-checked","true");}await call("updateSettings",{settings});}
});
$("#modalClose").addEventListener("click",closeModal); $("#modalLayer").addEventListener("click",e=>{if(e.target===$("#modalLayer"))closeModal();});

let libraryRefreshTimer=null;
function scheduleLibraryRefresh(){
  clearTimeout(libraryRefreshTimer);
  if(!state.libraryDirty)return;
  $("#libraryRefresh").hidden=false;
  if(state.scan?.active||["Settings","Audio","Lyrics","Queue","Now Playing"].includes(state.view))return;
  libraryRefreshTimer=setTimeout(async()=>{
    // Keep an expanded/paged list intact; the explicit refresh remains available.
    if(state.offset>0||state.modal||$("#modalLayer").hidden===false)return;
    state.libraryDirty=false;$("#libraryRefresh").hidden=true;
    await renderView(false,null,true);
  },1800);
}
$("#libraryRefresh").addEventListener("click",async()=>{state.libraryDirty=false;$("#libraryRefresh").hidden=true;await renderView(false,null,true);});
onEvent((name,data)=>{
  if(name==="windowRestored"){void (async()=>{await refreshCurrent();const fresh=await call("getBootstrap");state.settings={...state.settings,...fresh.settings};setTheme(state.settings);updateScanPresentation(fresh.scan);applyUpdateCheckState(!!fresh.updateCheckActive);state.latestRelease=fresh.latestRelease||null;state.updateAvailable=fresh.updateAvailable||null;if(fresh.libraryRevision!==state.libraryRevision){state.libraryRevision=fresh.libraryRevision;libraryViews.invalidateCache();state.libraryDirty=true;scheduleLibraryRefresh();}})();}
  else if(name==="playbackStateChanged"){
    const wasPlaying=state.playing,volume=state.volume;Object.assign(state,data,{position:Number(data.positionSeconds)||0,duration:Number(data.durationSeconds)||0});if(state.volumeEditing)state.volume=volume;state.positionUpdatedAt=performance.now();
    updatePlayer();
    if(!wasPlaying&&state.playing&&state.track&&state.lyricsTrack?.id!==state.track.id&&state.lyricsLoadingTrackId!==state.track.id)void loadLyricsForTrack(state.track.id,!!state.settings.autoLoadLyrics);
  }
  else if(name==="trackChanged"){const volume=state.volume;state.trackEventRevision=(state.trackEventRevision||0)+1;state.track=data.track||null;state.trackId=state.track?.id||"";state.lyricsLoadRevision++;state.lyricsLoadingTrackId="";state.lyricLines=[];state.lyricText="";state.lyricsTrack=null;state.lyricsRaw="";state.lyricsSource="none";state.lyricsAutoError="";state.lyricsRevision++;Object.assign(state,data,{position:Number(data.positionSeconds)||0,duration:Number(data.durationSeconds)||0});if(state.volumeEditing)state.volume=volume;state.positionUpdatedAt=performance.now();if(state.settings.autoOpenPanel&&!$("#appRoot").classList.contains("panel-compact"))setPanelOpen(true);updatePlayer();updatePanel();loadTrackDetails(state.track);if(state.track)void loadLyricsForTrack(state.track.id,!!state.settings.autoLoadLyrics);if(state.view==="Lyrics"||state.view==="Now Playing")renderView();}
  else if (name === "queueChanged") { state.queueEventRevision=(state.queueEventRevision||0)+1;state.queue = data.entries || []; state.queueOffset = data.queueOffset || 0; state.queueTotal = data.totalCount || 0; state.queueIndex = data.queueIndex ?? -1; updatePanel(); if (state.view === "Queue") { if (data.itemsChanged)void renderView(false,null,true); else updateQueueCurrentIndicator(); } }
  else if(name==="favoriteChanged"){const favorite=!!data.favorite;if(state.view==="Favorites"&&!favorite){const removedIndex=(state.items||[]).findIndex(track=>track.id===data.id);for(const row of $$("tr[data-track]"))if(row.dataset.track===data.id)row.remove();state.items=(state.items||[]).filter(track=>track.id!==data.id);state.total=Math.max(0,(state.total||0)-1);if(removedIndex>=0){state.nextOffset=Math.max(0,(state.nextOffset||0)-1);if(removedIndex<(state.offset||0))state.offset=Math.max(0,state.offset-1);}const subtitle=$("#routeView .page-head p");if(subtitle)subtitle.textContent=`${state.total.toLocaleString()} tracks`;}if(state.track?.id===data.id){state.track.favorite=favorite;updatePlayer();updatePanel();}for(const collection of [state.items,state.queue,state.queueItems])for(const track of collection||[])if(track.id===data.id)track.favorite=favorite;for(const button of $$('[data-action="favorite"]'))if(button.dataset.id===data.id){button.classList.toggle("on",favorite);button.setAttribute("aria-pressed",String(favorite));button.setAttribute("aria-label",favorite?"Remove from favorites":"Add to favorites");button.title=favorite?"Remove from favorites":"Add to favorites";if(!button.classList.contains("heart"))button.textContent=favorite?"♥ Favorited":"♡ Favorite";}}
  else if(name==="trackAvailabilityChanged"){if(state.track?.id===data.id)state.track.fileUnavailable=!!data.fileUnavailable;for(const track of state.queue||[])if(track.id===data.id)track.fileUnavailable=!!data.fileUnavailable;for(const track of state.queueItems||[])if(track.id===data.id)track.fileUnavailable=!!data.fileUnavailable;updatePlayer();updatePanel();if(state.view==="Queue")renderView();}
  else if(name==="scanChanged"){const wasActive=state.scan?.active;updateScanPresentation(data);if(wasActive&&!data.active)scheduleLibraryRefresh();}
  else if(name==="audioOutputChanged"){if(state.view==="Audio")void refreshAudioControls();}
  else if(name==="updateCheckState")applyUpdateCheckState(!!data.checking);
  else if(name==="latestReleaseChanged"){state.latestRelease=data.release||null;state.updateAvailable=data.release||null;if(state.view==="Settings")renderView();}
  else if(name==="libraryChanged"){
    state.libraryRevision=data.revision??((state.libraryRevision||0)+1);
    libraryViews.invalidateCache();
    if(data.reason!=="favorite"&&data.reason!=="listening"){state.libraryDirty=true;scheduleLibraryRefresh();}
  }
  else if(name==="defaultAppsChanged"){
    state.settings.needsDefaultSelection=data.needsDefaultSelection;
    const action=$("#chooseDefaultPlayer");if(action)action.hidden=data.needsDefaultSelection===false;
  }
  else if(name==="settingsChanged"){libraryViews.invalidateCache();const shouldLoadLyrics=!state.settings.autoLoadLyrics&&!!data.autoLoadLyrics;state.settings={...state.settings,...data};setTheme(state.settings);if(shouldLoadLyrics&&state.track)void loadLyricsForTrack(state.track.id,true);}
  else if(name==="artworkAccentChanged"){state.settings.artworkAccent=data.color||"";state.settings.artworkPalette=data.palette||null;setTheme(state.settings);}
  else if(name==="notification")toast(data.message||"");
  else if(name==="updateAvailable"){state.updateAvailable=data;state.latestRelease=data;if(state.view==="Settings")renderView();openModal(`Music Player ${esc(data.tag||"")} is available`,"Updates are opened in your browser and are never installed automatically.",`<button class="action" data-modal-close>Later</button><button class="action primary" data-action="open-release" data-url="${esc(data.url||"")}">View release</button>`);}
});

async function start() {
  installSelects();
  paintIcons();
  $$("#navigation .nav-item[data-view]").forEach(item=>{const label=item.querySelector("span:not([data-icon])")?.textContent?.trim();if(label){item.setAttribute("aria-label",label);item.title=label;}});
  try {
    const data=await call("getBootstrap");
    state.version=data.version;state.libraryRevision=data.libraryRevision||0;
    state.track=data.track||null; state.videoSupportEnabled=!!data.videoSupportEnabled;
    state.playing=!!data.playing; state.position=data.positionSeconds||0; state.duration=data.durationSeconds||0;
    state.volume=data.volume??75; state.positionUpdatedAt=performance.now(); state.lastVolume=state.volume||75; state.muted=state.volume===0;
    state.shuffle=!!data.shuffle; state.repeat=data.repeat||"Off"; state.repeatA=data.repeatA??null; state.repeatB=data.repeatB??null;
    state.queue=data.queue||[]; state.queueOffset=data.queueOffset||0; state.queueTotal=data.queueTotal||0; state.queueIndex=data.queueIndex??-1;
    state.panel=data.panel||"queue";
    state.settings={...(data.settings||{}),resolvedTheme:data.resolvedTheme||"Dark"};
    state.panelOpen=data.panelOpen??state.settings.rightPanelOpen??(window.innerWidth>1180);
    state.updateCheckActive=!!data.updateCheckActive; state.latestRelease=data.latestRelease||null; state.updateAvailable=data.updateAvailable||null;
    state.scan=data.scan||null; state.view=data.view||"Home"; state.search=data.search||""; state.group=data.group||null; state.playlist=data.playlist||null;
    if(state.view==="Search")$("#globalSearch").value=state.search;
    [state.sort,state.descending]=({"Most Played":["PlayCount",true],"Recently Played":["LastPlayed",true],"Recently Added":["Added",true]})[state.view]||["Title",false];
    state.trackId=state.track?.id||"";setTheme(state.settings); setPanelOpen(state.panelOpen,false);updatePlayer();updatePanel();
    const initialView=renderView();updateScanPresentation(state.scan);loadTrackDetails(state.track);if(state.track)void loadLyricsForTrack(state.track.id,!!state.settings.autoLoadLyrics);
    await initialView;
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    await call("uiReady");
    if(data.updateAvailable)openModal(`Music Player ${esc(data.updateAvailable.tag||"")} is available`,"Updates are opened in your browser and are never installed automatically.",`<button class="action" data-modal-close>Later</button><button class="action primary" data-action="open-release" data-url="${esc(data.updateAvailable.url||"")}">View release</button>`);
  }
  catch(error) { $("#routeView").innerHTML=`<div class="empty-state"><b>Music Player could not connect to Windows</b>${esc(error.message)}</div>`; }
}
start();

document.addEventListener("pointerdown",event=>{
  if(state.panelCompactExpanded&&!$("#rightPanel").contains(event.target)&&!$("#panelToggleBtn").contains(event.target)&&$("#modalLayer").hidden&&!event.target.closest(".context-menu,.select-popup"))setCompactPanelExpanded(false);
});
