import { activeLyricIndex } from "./lyrics.js";
import { patchMarkup } from "./view-dom.js";

export function createPlayerUi({state,$,$$,call,command,cover,esc,fmtDuration,svg,lyricLinesMarkup,loadLyricsForTrack}) {
  const trackDetailsCache = new Map();
  let trackDetailsRequest = 0;
  let trackDetailsTrackId = null;
  let panelCurrentTrackId = null;
  let displayedTrackId = null;
  let immersiveLyricsRenderKey = "";
  let playerRenderKey = "";
  let panelRenderKey = "";
  let volumeRenderKey = "";
function currentPosition(){return state.playing?Math.min(state.duration||Infinity,state.position+Math.max(0,performance.now()-(state.positionUpdatedAt||performance.now()))/1000):state.position;}
function renderProgress(){const position=currentPosition(),seek=$("#progressRange");$("#currentTime").textContent=fmtDuration(state.seekPreviewSeconds??position);$("#totalTime").textContent=fmtDuration(state.duration);if(!state.seeking&&!seek.matches(":active")){seek.max=String(Math.max(1,state.duration));seek.value=String(Math.min(state.duration,position));}seek.style.setProperty("--range-progress",`${Math.max(0,Math.min(100,Number(seek.value)/Math.max(1,Number(seek.max)||1)*100))}%`);const fullSeek=$("#immersiveProgress");if(fullSeek){fullSeek.max=seek.max;if(!state.seeking&&!fullSeek.matches(":active"))fullSeek.value=seek.value;fullSeek.style.setProperty("--range-progress",`${Math.max(0,Math.min(100,Number(fullSeek.value)/Math.max(1,Number(fullSeek.max)||1)*100))}%`);$("#immersiveCurrentTime").textContent=fmtDuration(state.seekPreviewSeconds??position);$("#immersiveTotalTime").textContent=fmtDuration(state.duration);}updateActiveLyric();}
function updatePlayer() {
  syncProgressTimer();
  renderVolume();
  const t=state.track;
  const key=[t?.id,t?.title,t?.artist,t?.album,t?.artworkUrl,t?.favorite,t?.fileUnavailable,state.playing,state.shuffle,state.repeat,state.repeatA,state.repeatB,state.lyricsRevision,state.lyricsTrack?.id,state.lyricsLoadingTrackId,state.lyricsAutoError,state.immersiveLyricsOpen].join("\u001f");
  if(key===playerRenderKey){if(!document.hidden)renderProgress();return;}
  playerRenderKey=key;
  const nextTrackId=t?.id||"__empty__";
  const coverEl=$("#nowCover");
  if(t) {
    $("#nowTitle").textContent=t.title;
    $("#nowArtist").textContent=[t.artist,t.album,t.fileUnavailable?"File unavailable":null].filter(Boolean).join(" · ");
    if(coverEl.dataset.trackId!==t.id||coverEl.dataset.artworkUrl!==(t.artworkUrl||"")){
      const holder=document.createElement("div");holder.innerHTML=cover(t);const art=holder.firstElementChild;
      coverEl.className=`now-cover cover ${[...art.classList].filter(x=>x.startsWith("c")).join(" ")}`;
      coverEl.innerHTML=art.innerHTML;coverEl.dataset.trackId=t.id;coverEl.dataset.artworkUrl=t.artworkUrl||"";
    }
    $("#immersiveTitle").textContent=t.title||"Unknown track";
    $("#immersiveArtist").textContent=t.artist||"Unknown artist";
    const backdrop=$("#immersiveBackdrop");backdrop.hidden=!t.artworkUrl;if(t.artworkUrl){if(backdrop.getAttribute("src")!==t.artworkUrl)backdrop.src=t.artworkUrl;}else backdrop.removeAttribute("src");
    const immersiveArt=$("#immersiveArtwork");if(immersiveArt.dataset.trackId!==t.id||immersiveArt.dataset.artworkUrl!==(t.artworkUrl||"")){immersiveArt.innerHTML=cover(t,"immersive-cover");immersiveArt.dataset.trackId=t.id;immersiveArt.dataset.artworkUrl=t.artworkUrl||"";const image=immersiveArt.querySelector("img");if(image)image.loading="eager";}
  } else {
    $("#nowTitle").textContent="Nothing playing";
    $("#nowArtist").textContent="Choose a song from your library";
    if(coverEl.dataset.trackId!=="__empty__") {
      coverEl.className="now-cover cover c4";
      coverEl.innerHTML='<div class="cover-art">MP</div>';
      coverEl.dataset.trackId="__empty__";
    }
    $("#immersiveTitle").textContent="Nothing playing";$("#immersiveArtist").textContent="Choose a song from your library";$("#immersiveBackdrop").hidden=true;$("#immersiveArtwork").innerHTML=cover({title:"Music Player"},"immersive-cover");$("#immersiveArtwork").dataset.trackId="__empty__";
  }
  $("#playBtn").dataset.icon=state.playing?"pause":"play"; $("#playBtn").title=state.playing?"Pause":"Play"; $("#playBtn").setAttribute("aria-label",state.playing?"Pause":"Play"); $("#playBtn").setAttribute("aria-pressed",String(state.playing)); $("#playBtn").innerHTML=svg(state.playing?"pause":"play");
  const shuffleBtn=$("#shuffleBtn"); shuffleBtn.classList.toggle("active",state.shuffle); shuffleBtn.title=state.shuffle?"Shuffle on":"Shuffle off"; shuffleBtn.setAttribute("aria-label",shuffleBtn.title); shuffleBtn.setAttribute("aria-pressed",String(state.shuffle));
  const repeatBtn=$("#repeatBtn"); const repeatLabel=state.repeat==="Queue"?"Repeat all":state.repeat==="Track"?"Repeat one":"Repeat off"; repeatBtn.classList.toggle("active",state.repeat!=="Off"); repeatBtn.title=repeatLabel; repeatBtn.setAttribute("aria-label",repeatLabel); repeatBtn.setAttribute("aria-pressed",String(state.repeat!=="Off")); repeatBtn.innerHTML=`${svg("repeat")}${state.repeat==="Track"?'<span class="repeat-one" aria-hidden="true">1</span>':""}`;
  const abBtn=$("#abBtn"); abBtn.classList.toggle("active",state.repeatA!==null); abBtn.title=state.repeatB!==null?"A–B repeat on":state.repeatA!==null?"Mark point B":"A–B repeat off"; abBtn.setAttribute("aria-label",abBtn.title); abBtn.setAttribute("aria-pressed",String(state.repeatA!==null));
  const favoriteLabel=t?.favorite?"Remove from favorites":"Add to favorites";
  const heartBtn=$("#heartBtn"); heartBtn.classList.toggle("on",!!t?.favorite); heartBtn.title=favoriteLabel; heartBtn.setAttribute("aria-label",favoriteLabel); heartBtn.setAttribute("aria-pressed",String(!!t?.favorite)); heartBtn.innerHTML=svg("heart");
  const fullPlay=$("#immersivePlay");fullPlay.innerHTML=svg(state.playing?"pause":"play");fullPlay.title=state.playing?"Pause":"Play";fullPlay.setAttribute("aria-label",fullPlay.title);
  const fullHeart=$("#immersiveHeart");fullHeart.classList.toggle("on",!!t?.favorite);fullHeart.title=favoriteLabel;fullHeart.setAttribute("aria-label",favoriteLabel);fullHeart.setAttribute("aria-pressed",String(!!t?.favorite));fullHeart.innerHTML=svg("heart");
  const fullToggle=$("#immersiveToggle");fullToggle.disabled=!t;fullToggle.setAttribute("aria-disabled",String(!t));
  renderImmersiveLyrics();
  if(nextTrackId!==displayedTrackId){displayedTrackId=nextTrackId;animateTrackChange();}
  renderProgress();
}
function renderVolume(){
  if(state.volume>0){state.lastVolume=state.volume;state.muted=false;}else state.muted=true;
  const audioVolume=$("#audioVolume");if(audioVolume&&!audioVolume.matches(":active"))audioVolume.value=String(state.volume);
  const key=`${state.volume}:${state.muted}`;if(key===volumeRenderKey)return;volumeRenderKey=key;
  const range=$("#volumeRange");if(!range.matches(":active"))range.value=String(state.volume);range.style.setProperty("--range-progress",`${Math.max(0,Math.min(100,state.volume))}%`); const muteBtn=$("#muteBtn"); muteBtn.title=state.muted?"Unmute":"Mute"; muteBtn.setAttribute("aria-label",muteBtn.title); muteBtn.setAttribute("aria-pressed",String(state.muted)); muteBtn.innerHTML=svg(state.muted?"mute":"volume");
}

function animateTrackChange(){
  if(state.settings.motionStyle==="Off"||matchMedia("(prefers-reduced-motion: reduce)").matches)return;
  for(const element of [$("#nowCover"),$("#nowTitle"),$("#immersiveArtwork"),$("#immersiveTitle")]){
    if(!element)continue;
    element.classList.remove("track-enter");void element.offsetWidth;element.classList.add("track-enter");
    setTimeout(()=>element.classList.remove("track-enter"),500);
  }
}

function renderImmersiveLyrics(){
  const current=state.track&&state.lyricsTrack?.id===state.track.id;
  const revisionKey=[state.immersiveLyricsOpen,state.track?.id||"",state.lyricsTrack?.id||"",state.lyricsRevision||0,state.lyricsLoadingTrackId||"",state.lyricsAutoError||""].join("|");
  if(revisionKey===immersiveLyricsRenderKey)return;
  immersiveLyricsRenderKey=revisionKey;
  const lines=$("#immersiveLyricsLines");if(!lines)return;
  const source=state.lyricsSource==="lrclib"?"LRCLIB (online)":state.lyricsSource==="embedded"?"Embedded audio tags":state.lyricsSource==="sidecar"?"Sidecar file":"";
  $("#immersiveLyricsSource").textContent=current&&source?`· ${source}`:"";
  if(!current)lines.innerHTML=`<div class="empty-state"><b>Loading lyrics…</b>Saved lyrics appear here when they are ready.</div>`;
  else if(state.lyricLines?.length)lines.innerHTML=lyricLinesMarkup(state.lyricLines,esc,state.settings.seekFromLyrics!==false);
  else if(state.lyricText)lines.innerHTML=`<p class="lyric-source">Untimed lyrics · automatic following requires timestamps.</p><div class="lyrics-plain">${esc(state.lyricText)}</div>`;
  else if(state.lyricsLoadingTrackId===state.track.id)lines.innerHTML=`<div class="empty-state"><b>Finding lyrics…</b>${state.settings.autoLoadLyrics?"Checking for a matching LRCLIB result.":"Loading lyrics saved with this track."}</div>`;
  else lines.innerHTML=`<div class="empty-state"><b>No lyrics found</b>${esc(state.lyricsAutoError||"Search LRCLIB or edit lyrics to add them.")}</div>`;
}

function updateActiveLyric(){
  const trackId=state.lyricsTrack?.id;
  const canFollow=state.lyricLines.length>0&&!!trackId&&trackId===state.track?.id;
  let active=-1;
  if(canFollow)active=activeLyricIndex(state.lyricLines,currentPosition());
  const canFollowMain=canFollow&&state.view==="Lyrics";
  const canFollowImmersive=canFollow&&state.immersiveLyricsOpen&&!$("#immersivePlayer").hidden;
  const roots=[{element:$("#lyricsLines"),enabled:canFollowMain,scroller:$("#content")},{element:$("#immersiveLyricsLines"),enabled:canFollowImmersive,scroller:$("#immersiveLyricsScroll")}];
  for(const {element,enabled,scroller} of roots){
    if(!element)continue;
    const key=`${trackId||""}:${active}:${state.lyricsRevision||0}:${enabled}:${scroller?.clientWidth}:${scroller?.clientHeight}`;
    if(element.dataset.activeLyricKey===key)continue;
    element.dataset.activeLyricKey=key;
    const nodes=$$("[data-lyric-index]",element);
    for(const node of nodes){const index=Number(node.dataset.lyricIndex);node.classList.toggle("active",enabled&&index===active);node.classList.toggle("past",enabled&&index<active);node.classList.toggle("upcoming",enabled&&(active<0||index>active));}
    if(!enabled||active<0||!scroller)continue;
    const activeElement=element.querySelector(`[data-lyric-index="${active}"]`);if(!activeElement)continue;
    const targetTop=activeElement.offsetTop-(scroller.clientHeight-activeElement.offsetHeight)/2;
    scroller.scrollTo({top:Math.max(0,targetTop),behavior:state.settings.motionStyle==="Off"||matchMedia("(prefers-reduced-motion: reduce)").matches?"auto":"smooth"});
  }
}

function updatePanel() {
  const key=JSON.stringify([state.panel,state.track,state.trackDetails,state.queueIndex,state.queueOffset,state.queue]);
  if(key===panelRenderKey)return;
  panelRenderKey=key;
  const current=state.track; const currentBox=$("#panelCurrent"); const content=$("#panelContent");
  const currentId=current?.id||null;if(currentId!==panelCurrentTrackId){panelCurrentTrackId=currentId;$("#panelScroll").scrollTop=0;}
  if(!current) patchMarkup(currentBox,`<div class="empty-state"><b>Nothing playing</b>Choose a track to see details here.</div>`);
  else { const favoriteLabel=current.favorite?"Remove from favorites":"Add to favorites"; patchMarkup(currentBox,`${cover(current,"panel-cover")}<div class="panel-track-title"><div><h2>${esc(current.title)}</h2><p>${esc(current.artist)}${current.album?` · ${esc(current.album)}`:""}${current.fileUnavailable?" · File unavailable":""}</p></div><button class="ctrl heart ${current.favorite?"on":""}" data-action="favorite" data-id="${esc(current.id)}" title="${favoriteLabel}" aria-label="${favoriteLabel}" aria-pressed="${current.favorite?"true":"false"}">${svg("heart")}</button></div>`); }
  const quality=[];
  const bitrate=Number(state.trackDetails?.bitrateKbps)||0;
  const sampleRate=Number(state.trackDetails?.sampleRateHz)||0;
  const bits=Number(state.trackDetails?.bitsPerSample)||0;
  if(bitrate>0)quality.push(`${bitrate} kbps`);
  if(sampleRate>0)quality.push(sampleRate>=1000?`${(sampleRate/1000).toFixed(sampleRate%1000===0?0:1)} kHz`:`${sampleRate} Hz`);
  if(bits>0)quality.push(`${bits}-bit`);
  const qualityText=quality.join(" · ");
  const qualityRow=quality.length?`<div class="info-row"><span>Audio quality</span><b title="${esc(qualityText)}">${esc(qualityText)}</b></div>`:"";
  if(state.panel==="queue") {
    const start=state.queueIndex-(state.queueOffset||0);
    const upcoming=state.queue.slice(start+1,start+4);
    patchMarkup(content,`<div class="panel-meta">${current?`<div class="info-row"><span>Album</span><b title="${esc(current.album||"—")}">${esc(current.album||"—")}</b></div><div class="info-row"><span>Format</span><b title="${esc(current.format||"—")}">${esc(current.format||"—")}</b></div>${current.fileUnavailable?`<div class="info-row"><span>Status</span><b>File unavailable</b></div>`:""}${qualityRow}<div class="info-row"><span>Location</span><b class="location" data-action="copy-location" data-id="${esc(current.id)}" title="${esc(current.path||"")}" role="button" tabindex="0" aria-label="Copy full file path">${esc(current.path||"—")}</b></div>`:`<div class="empty-state">Track information appears here.</div>`}</div><div class="panel-label">Next in queue</div>${upcoming.length?upcoming.map((t,i)=>`<div class="queue-row ${t.fileUnavailable?"unavailable-track":""}" data-action="play-queue" data-index="${state.queueOffset+start+i+1}" data-context="track" data-context-id="${esc(t.id)}" role="button" tabindex="0" aria-label="Play ${esc(t.title)}">${cover(t,"queue-thumb")}<div class="queue-copy"><b>${esc(t.title)}</b><span>${esc(t.fileUnavailable?(t.artist==="Unavailable"?"File unavailable":`${t.artist} · File unavailable`):t.artist)}</span></div><span class="row-tools"><button class="ctrl" data-action="queue-remove" data-index="${state.queueOffset+start+i+1}" aria-label="Remove from queue">×</button></span></div>`).join(""):`<div class="empty-state">No upcoming tracks.</div>`}`);
  }
  else patchMarkup(content,current?`<div class="info-block"><h3>Track information</h3><div class="info-row"><span>Album</span><b>${esc(current.album||"—")}</b></div><div class="info-row"><span>Album artist</span><b>${esc(current.albumArtist||current.artist||"—")}</b></div><div class="info-row"><span>Year · Genre</span><b>${esc([current.year,current.genre].filter(Boolean).join(" · ")||"—")}</b></div><div class="info-row"><span>Audio format</span><b>${esc(current.format||"—")}</b></div>${qualityRow}</div><div class="toolbar"><button class="action" data-action="details" data-id="${esc(current.id)}">Track details</button><button class="action" data-action="show-location" data-id="${esc(current.id)}">Show in File Explorer</button></div>`:`<div class="empty-state">Track information appears here.</div>`);
  $$(".panel-tabs [data-panel]").forEach(b=>b.classList.toggle("active",b.dataset.panel===state.panel));
}

async function loadTrackDetails(track) {
  const id=track?.id||"";
  if(id===trackDetailsTrackId)return;
  trackDetailsTrackId=id;
  const request=++trackDetailsRequest;
  state.trackDetails=null;
  if(!track?.id){updatePanel();return;}
  const cached=trackDetailsCache.get(track.id);
  if(cached){state.trackDetails=cached;updatePanel();return;}
  try {
    const details=await command("getTrackDetails",{id:track.id});
    if(request!==trackDetailsRequest||state.track?.id!==track.id)return;
    const projected={bitrateKbps:details.bitrateKbps,sampleRateHz:details.sampleRateHz,bitsPerSample:details.bitsPerSample};
    trackDetailsCache.set(track.id,projected);
    if(trackDetailsCache.size>32)trackDetailsCache.delete(trackDetailsCache.keys().next().value);
    state.trackDetails=projected;updatePanel();
  } catch { /* Audio properties are optional; full track details remain available on demand. */ }
}

let currentRequest=0;
async function refreshCurrent() {
  const request=++currentRequest,trackRevision=state.trackEventRevision||0,queueRevision=state.queueEventRevision||0;
  const previousTrackId=state.track?.id,data=await call("getCurrentTrack");
  if(request!==currentRequest||trackRevision!==(state.trackEventRevision||0))return;
  state.track=data.track||null;
  const currentTrackId=state.track?.id||"";
  if(currentTrackId!==previousTrackId){
    state.trackId=currentTrackId;state.lyricsLoadRevision++;state.lyricsLoadingTrackId="";state.lyricsTrack=null;state.lyricLines=[];state.lyricText="";state.lyricsRaw="";state.lyricsSource="none";state.lyricsAutoError="";state.lyricsRevision++;
  }
  state.playing=!!data.playing;state.position=data.positionSeconds||0;state.positionUpdatedAt=performance.now();state.duration=data.durationSeconds||0;if(!state.volumeEditing)state.volume=data.volume??state.volume;state.shuffle=!!data.shuffle;state.repeat=data.repeat||"Off";state.repeatA=data.repeatA??null;state.repeatB=data.repeatB??null;if(queueRevision===(state.queueEventRevision||0))state.queueIndex=data.queueIndex??-1;
  const offset=Math.max(0,state.queueIndex-1),q=await call("getQueue",{offset,pageSize:100});
  if(request!==currentRequest||trackRevision!==(state.trackEventRevision||0))return;
  if(queueRevision===(state.queueEventRevision||0)){state.queue=q.entries||[];state.queueOffset=q.queueOffset??offset;state.queueTotal=q.totalCount||0;state.queueIndex=q.queueIndex??state.queueIndex;}
  updatePlayer();updatePanel();
  if(state.track&&((state.track.id!==previousTrackId)||(state.lyricsTrack?.id!==state.track.id&&state.lyricsLoadingTrackId!==state.track.id)))void loadLyricsForTrack(state.track.id,!!state.settings.autoLoadLyrics);
}

let pendingVolume=null,volumeWrite=null;
async function setVolume(value){
  state.volumeEditing=true;state.volume=Math.max(0,Math.min(100,Number(value)||0));pendingVolume=state.volume;renderVolume();
  if(!volumeWrite)volumeWrite=(async()=>{try{while(pendingVolume!==null){const volume=pendingVolume;pendingVolume=null;await call("setVolume",{volume});}}finally{volumeWrite=null;state.volumeEditing=false;}})();
  await volumeWrite;
}
async function toggleMute(){if(state.muted){await setVolume(state.lastVolume||75);}else{state.lastVolume=state.volume||state.lastVolume||75;await setVolume(0);}}

  let progressTimer=null;
  function syncProgressTimer(){
    const active=state.playing&&!document.hidden;
    if(active&&progressTimer===null)progressTimer=setInterval(renderProgress,100);
    else if(!active&&progressTimer!==null){clearInterval(progressTimer);progressTimer=null;}
  }
  document.addEventListener("visibilitychange",()=>{syncProgressTimer();if(!document.hidden)void refreshCurrent();});
  return {updatePlayer,updateActiveLyric,updatePanel,loadTrackDetails,refreshCurrent,setVolume,toggleMute};
}
