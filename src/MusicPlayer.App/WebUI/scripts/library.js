import { createPageCache } from "./page-cache.js";
import { lyricLinesMarkup } from "./lyrics.js";
import { patchMarkup } from "./view-dom.js";

export function createLibraryViews({state,$,$$,svg,paintIcons,esc,initials,cover,titleOf,subOf,fmtDuration,bytesLabel,call,toast,setTheme,applyNavigationSettings,openModal,updatePlayer,updatePanel,ensureLyricsLoaded,lyricLinesMarkup:renderLyricLines=lyricLinesMarkup}) {
let renderRevision=0;
const pages=createPageCache(6);
const cacheable=new Set(["Home","Search","Songs","Albums","Artists","Genres","Folders","Favorites","Most Played","Recently Played","Recently Added","With Lyrics","Videos","Playlists","Playlist","Album","Artist","Genre","Folder"]);
const pageFields=["items","total","offset","nextOffset","artistAlbumItems","artistAlbumsOffset","artistAlbumsHaveMore","folderItems","folderOffset","folderCount","homeHasRoots","indexedBytes"];
let mountedKey=null;
let mountedView=null;
const viewSorts=new Map();
function defaultSort(view){return ({"Most Played":["PlayCount",true],"Recently Played":["LastPlayed",true],"Recently Added":["Added",true]})[view]||["Title",false];}
function rememberSort(){viewSorts.set(state.view,[state.sort,state.descending]);}
let releaseHistoryLoad=null;
function pageKey(){return JSON.stringify([state.view,state.search,state.group,state.playlist,state.sort,state.descending,state.searchFilter,state.libraryRevision||0,state.settings.songColumns,state.settings.artworkStyle,state.settings.seekFromLyrics]);}
function cacheMarkup(root){
  const clone=root.cloneNode(true);
  for(const wrapper of clone.querySelectorAll(".select-control")){
    const select=wrapper.querySelector("select");if(!select)continue;
    const live=[...root.querySelectorAll("select")].find(item=>select.id?item.id===select.id:item.dataset.setting===select.dataset.setting);
    for(const option of select.options)option.toggleAttribute("selected",option.value===live?.value);
    select.classList.remove("select-native");select.hidden=false;select.removeAttribute("tabindex");select.removeAttribute("aria-hidden");wrapper.replaceWith(select);
  }
  return clone.innerHTML;
}
function invalidateCache(){pages.clear();}
function capturePage(){
  ++renderRevision;pageLoad=null;
  const root=$("#routeView"),fragment=document.createDocumentFragment(),key=mountedKey,view=mountedView;
  const fields=[...pageFields,"view","search","group","playlist","sort","descending","queueItems","queuePageOffset"];
  const snapshot=Object.fromEntries(fields.map(field=>[field,state[field]]));
  fragment.append(...root.childNodes);
  return async()=>{
    ++renderRevision;pageLoad=null;Object.assign(state,snapshot);mountedKey=key;mountedView=view;root.replaceChildren(fragment);
    $("#globalSearch").value=state.search||"";
    $$(".nav-item").forEach(item=>item.classList.toggle("active",item.dataset.view===(state.view==="Playlist"?"Playlists":state.view)));
    $$(".player [data-view]").forEach(item=>item.classList.toggle("active",item.dataset.view===state.view));
    updatePlayer();updatePanel();
    await call("setView",{view:state.view,search:state.search,group:state.group,playlistId:state.playlist?.id||null});
    if(root.querySelector('.empty-state[aria-busy="true"]'))await renderView();
  };
}
function loadReleaseHistory(){
  if(releaseHistoryLoad||state.releaseHistory)return;
  releaseHistoryLoad=call("getReleaseHistory").then(releases=>{state.releaseHistory=releases;paintReleaseHistory();})
    .catch(()=>paintReleaseHistory(true)).finally(()=>{releaseHistoryLoad=null;});
}
function paintReleaseHistory(failed=false){
  const node=$("#releaseHistory");if(!node)return;
  node.innerHTML=state.releaseHistory?.length?state.releaseHistory.map(release=>`<button class="about-version" data-action="open-release" data-url="${esc(release.url)}"><b>${esc(release.tag)}</b><span>${release.prerelease?"Preview release":"Stable release"}</span></button>`).join(""):
    `<p class="muted">${failed?"Release history is unavailable offline.":"No published releases were found."} Use “View all releases” when connected.</p>`;
}
let pageLoad=null;
let searchRevision=0;
let searchRequestSerial=0;
function nextSearchRequestId(){return Date.now()*1000+(++searchRequestSerial%1000);}
function trackRow(track,index) {
  const id=esc(track.id); const album=esc(track.album||""); const artist=esc(track.artist||"");
  const remove=state.view==="Playlist"&&Number.isInteger(track.position)?`<button class="ctrl" data-action="remove-playlist-track" data-position="${track.position}" aria-label="Remove from playlist">×</button>`:"";
  const plays=state.view==="Most Played"?`<td class="play-count-col" data-column="plays" title="Played ${Number(track.playCount||0)} times">${Number(track.playCount||0).toLocaleString()}</td>`:"";
  const subtitle=track.fileUnavailable?(track.artist==="Unavailable"?"File unavailable":`${artist} · File unavailable`):artist;
  return `<tr data-track="${id}" tabindex="0" data-context="track" class="${track.fileUnavailable?"unavailable-track":""}"><td class="index">${index+1}</td><td data-column="title"><div class="song-cell">${cover(track,"song-thumb")}<div class="song-info"><b>${esc(track.title)}</b><span>${esc(subtitle)}</span></div><div class="row-actions"><button class="ctrl heart ${track.favorite?"on":""}" data-action="favorite" data-id="${id}" aria-label="${track.favorite?"Remove from favorites":"Add to favorites"}" aria-pressed="${track.favorite?"true":"false"}">${svg("heart")}</button>${remove}<button class="ctrl" data-action="context" data-id="${id}" aria-label="More track actions">⋯</button></div></div></td><td class="album-col" data-column="album">${album}</td><td class="added-col" data-column="added">${esc(track.addedDisplay||"")}</td><td class="year-col" data-column="year">${track.year||""}</td>${plays}<td class="time" data-column="duration">${fmtDuration(track.durationSeconds)}</td></tr>`;
}
function songTable(tracks, start=0) {
  if (!tracks?.length) {
    const empty={
      "Favorites":["No favorites yet","Use the heart on a track to keep it here."],
      "Most Played":["Nothing played often yet","Play music from your library to build this list."],
      "Recently Played":["Nothing played recently","Tracks you play will appear here."],
      "Videos":["No videos indexed yet","Enable a video extension in Scan exclusions, then scan your folders."],
      "Recently Added":["No recent additions","Newly indexed tracks will appear here."],
      "Album":["No indexed tracks for this album","Rescan its folder if the files are available."],
      "Artist":["No indexed tracks for this artist","Rescan its folders if the files are available."],
      "Genre":["No indexed tracks for this genre","Try another genre or rescan the library."]
    }[state.view]||["No tracks here yet","Music in your indexed folders will appear here."];
    return `<div class="empty-state"><b>${empty[0]}</b>${empty[1]}</div>`;
  }
  const s=state.settings;const classes=[s.showArtwork===false?"no-artwork":"",s.showArtist===false?"no-artists":"",s.showAlbum===false?"no-albums":"",s.showAdded===false?"no-added":"show-added",s.showYear?"show-years":"",s.showDuration===false?"no-duration":"",s.showFavorite===false?"no-favorite":""].filter(Boolean).join(" ");
  return `<div class="table-wrap"><table class="song-table ${classes} ${state.view==="Most Played"?"most-played-table":""}"><thead><tr><th class="index">#</th><th data-column="title"><button data-sort="Title">Title</button><span class="column-resize" data-column-resize="title" role="separator" aria-orientation="vertical" aria-label="Resize title column" title="Drag to resize"></span></th><th class="album-col" data-column="album"><button data-sort="Album">Album</button><span class="column-resize" data-column-resize="album" role="separator" aria-orientation="vertical" aria-label="Resize album column" title="Drag to resize"></span></th><th class="added-col" data-column="added"><button data-sort="Added">Added</button><span class="column-resize" data-column-resize="added" role="separator" aria-orientation="vertical" aria-label="Resize added column" title="Drag to resize"></span></th><th class="year-col" data-column="year"><button data-sort="Year">Year</button><span class="column-resize" data-column-resize="year" role="separator" aria-orientation="vertical" aria-label="Resize year column" title="Drag to resize"></span></th>${state.view==="Most Played"?`<th class="play-count-col" data-column="plays"><button data-sort="PlayCount">Plays</button><span class="column-resize" data-column-resize="plays" role="separator" aria-orientation="vertical" aria-label="Resize plays column" title="Drag to resize"></span></th>`:""}<th class="time" data-column="duration"><button data-sort="Duration">Time</button><span class="column-resize" data-column-resize="duration" role="separator" aria-orientation="vertical" aria-label="Resize time column" title="Drag to resize"></span></th></tr></thead><tbody>${tracks.map((t,i)=>trackRow(t,start+i)).join("")}</tbody></table></div>`;
}
function card(item,type="album") {
  const clean=value=>{const text=String(value??"").trim();return text&&!/^(?:undefined|null)$/i.test(text)?text:"";};
  const name=clean(item.name)||clean(item.title)||titleOf(item);
  const id=esc(clean(item.id)||name); const key=esc(name);
  if(type==="track") return `<article class="card ${item.fileUnavailable?"unavailable-track":""}" tabindex="0" data-action="play" data-id="${id}">${cover(item)}<div class="card-title">${esc(titleOf(item))}</div><div class="card-sub">${esc(item.fileUnavailable?"File unavailable":subOf(item))}</div><button class="play-fab" data-action="play" data-id="${id}" title="Play">${svg("play")}</button></article>`;
  const playAction=type==="playlist"?"play-playlist-card":"play-group";
  return `<article class="card" tabindex="0" data-open-type="${type}" data-open-id="${id}" data-open-name="${type==="folder"?id:key}">${cover(item,type==="artist"?"round":"")}<div class="card-title">${esc(titleOf(item))}</div><div class="card-sub">${esc(subOf(item) || `${item.trackCount||item.count||0} tracks`)}</div><button class="play-fab" data-action="${playAction}" data-type="${type}" data-id="${id}" title="Play">${svg("play")}</button></article>`;
}
function cardGrid(items,type="album") {
  if(items?.length) return `<div class="card-grid">${items.map(item=>card(item,type)).join("")}</div>`;
  const empty=state.view==="Playlists"?["No playlists yet","Create a playlist or import an M3U file."]:
    state.view==="Albums"?["No albums indexed","Add or rescan a music folder to find albums."]:
    state.view==="Artists"?["No artists indexed","Add or rescan a music folder to find artists."]:
    state.view==="Genres"?["No genres indexed","Add or rescan a music folder to find genres."]:
    state.view==="Folders"?["No indexed folders","Add a library root and scan it to see folders."]:
    ["Nothing to show yet","Your library will appear here after a folder is scanned."];
  return `<div class="empty-state"><b>${empty[0]}</b>${empty[1]}</div>`;
}
function section(title,items,type,view) { return `<section class="section"><div class="section-head"><h2>${esc(title)}</h2>${view?`<button data-view="${esc(view)}">See all</button>`:""}</div>${type==="track"?`<div class="mini-row-grid">${(items||[]).map((t,i)=>`<div class="mini-row ${t.fileUnavailable?"unavailable-track":""}" data-track="${esc(t.id)}" data-context="track">${cover(t,"mini-cover")}<div class="mini-meta"><b>${esc(t.title)}</b><span>${esc(t.fileUnavailable?"File unavailable":t.artist)}</span></div><button class="ctrl" data-action="play" data-id="${esc(t.id)}" aria-label="Play">${svg("play")}</button></div>`).join("")}</div>`:cardGrid(items,type||"album")}</section>`; }
function viewHeader(title,subtitle,actions="") { return `<div class="page-head"><div><h1>${esc(title)}</h1><p>${esc(subtitle||"")}</p></div>${actions}</div>`; }
function toolbar(actions="") { return `<div class="toolbar">${actions}</div>`; }

async function navigate(view,push=true,query=undefined) {
  rememberSort();
  [state.sort,state.descending]=viewSorts.get(view)||defaultSort(view);
  if(state.view==="Search"&&view!=="Search") await call("beginSearch",{requestId:nextSearchRequestId()},false);
  if (push && state.view!==view) { state.history=state.history.slice(0,state.historyIndex+1); state.history.push(view); state.historyIndex=state.history.length-1; }
  state.view=view; state.offset=0; if(!["Album","Artist","Genre","Folder"].includes(view)) state.group=null; if(view!=="Playlist") state.playlist=null; if(query!==undefined) state.search=query;
  const navView=view==="Playlist"?"Playlists":view;
  $$(".nav-item").forEach(item=>item.classList.toggle("active",item.dataset.view===navView));
  $$(".player [data-view]").forEach(item=>item.classList.toggle("active",item.dataset.view===view));
  const content=$("#content"); content.scrollTop=0;
  if(["Songs","Albums","Artists","Genres","Folders","Favorites","Most Played","Recently Played","Recently Added","With Lyrics","Videos","Playlists","Queue","Audio","Settings","Duplicates","Lyrics","Now Playing"].includes(view)) state.search="";
  if(view!=="Search" && $("#globalSearch").value) { $("#globalSearch").value=""; state.search=""; }
  const currentSearch=state.search,currentGroup=state.group,currentPlaylistId=state.playlist?.id||null;
  const contextUpdate=call("setView",{view,search:currentSearch,group:currentGroup,playlistId:currentPlaylistId});
  const rendering=renderView();
  await contextUpdate;
  if(state.view!==view||state.search!==currentSearch||state.group!==currentGroup||(state.playlist?.id||null)!==currentPlaylistId)return;
  await rendering;
}

async function renderView(append=false,previousButton=null,background=false) {
  const started=performance.now();
  const report=cached=>{performance.clearMarks("music-player-navigation");performance.mark("music-player-navigation",{detail:{view:state.view,cached,durationMs:performance.now()-started,cacheSize:pages.size}});};
  const revision=++renderRevision;
  const requestedView=state.view;
  const root=$("#routeView");
  const key=pageKey();
  if(!append) {
    state.offset=0;state.nextOffset=0;pageLoad=null;
    const cached=cacheable.has(requestedView)?pages.get(key):null;
    if(cached){Object.assign(state,cached.state);if(mountedKey!==key){root.innerHTML=cached.html;paintIcons(root);}mountedKey=key;mountedView=requestedView;updatePanel();updatePlayer();report(true);return true;}
    if(!background&&mountedView!==requestedView)root.innerHTML=`<div class="empty-state" role="status" aria-busy="true"><b>Loading ${esc(requestedView.toLowerCase())}…</b></div>`;
  }
  try {
    let html="";
    switch(state.view) {
      case "Home": html=await renderHome(); break;
      case "Search": html=await renderSearch(); break;
      case "Songs": case "Favorites": case "Most Played": case "Recently Played": case "Recently Added": case "With Lyrics": case "Videos": html=await renderTracksView(append); break;
      case "Albums": case "Artists": case "Genres": html=await renderGroupsView(append); break;
      case "Folders": html=await renderFolders(append); break;
      case "Playlists": html=await renderPlaylists(); break;
      case "Playlist": html=await renderPlaylistDetail(append); break;
      case "Album": case "Artist": case "Genre": case "Folder": html=await renderGroupDetail(append); break;
      case "Queue": html=await renderQueue(append,background); break;
      case "Now Playing": html=await renderNowPlaying(); break;
      case "Lyrics": html=await renderLyrics(); break;
      case "Audio": html=await renderAudio(); break;
      case "Settings": html=await renderSettings(); break;
      case "Duplicates": html=await renderDuplicates(); break;
      default: html=await renderHome();
    }
    if(revision!==renderRevision||requestedView!==state.view||html==null)return false;
    if(append) {if(previousButton){previousButton.insertAdjacentHTML("beforebegin",html);previousButton.remove();}else root.insertAdjacentHTML("beforeend",html);} else if(background&&mountedView===requestedView)patchMarkup(root,html);else root.innerHTML=html;
    mountedKey=key;
    mountedView=requestedView;
    if(cacheable.has(requestedView))pages.set(key,{html:cacheMarkup(root),state:Object.fromEntries(pageFields.map(field=>[field,Array.isArray(state[field])?[...state[field]]:state[field]]))});
    paintIcons(root); updatePanel(); updatePlayer();
    if(requestedView==="Settings"){if(state.releaseHistory)paintReleaseHistory();else loadReleaseHistory();}
    report(false);
    return true;
  } catch (error) { if(revision===renderRevision&&requestedView===state.view) {if(append)toast(`Could not load more: ${error.message}`);else root.innerHTML=`<div class="empty-state"><b>Could not load this view</b>${esc(error.message)}</div>`;}return false; }
}

async function loadMore(button) {
  if(button.disabled||pageLoad)return false;
  const previousOffset=state.offset;
  const request={revision:renderRevision+1};pageLoad=request;
  button.disabled=true;button.setAttribute("aria-busy","true");
  state.offset=state.nextOffset;
  try {
    const loaded=await renderView(true,button);
    if(!loaded&&request.revision===renderRevision)state.offset=previousOffset;
    return loaded;
  } finally {if(pageLoad===request)pageLoad=null;button.disabled=false;button.removeAttribute("aria-busy");}
}

async function loadArtistAlbums(button) {
  if(button.disabled)return;
  const revision=renderRevision,group=state.group;
  button.disabled=true;button.setAttribute("aria-busy","true");
  try {
    const page=await call("getArtistAlbums",{artist:group?.name||"",offset:state.artistAlbumsOffset||0,pageSize:30});
    if(revision!==renderRevision||group!==state.group||state.view!=="Artist")return;
    const groups=page.groups||[];
    state.artistAlbumItems=[...(state.artistAlbumItems||[]),...groups];
    state.artistAlbumsOffset=(state.artistAlbumsOffset||0)+groups.length;
    if(groups.length)$("#artistAlbumsGrid")?.insertAdjacentHTML("beforeend",groups.map(item=>card(item,"album")).join(""));
    state.artistAlbumsHaveMore=page.hasMore===true;
    if(!state.artistAlbumsHaveMore)button.remove();
    paintIcons($("#artistAlbumsGrid"));
  } catch(error) {if(revision===renderRevision)toast(`Could not load more albums: ${error.message}`);}
  finally {button.disabled=false;button.removeAttribute("aria-busy");}
}

async function renderHome() {
  const revision=renderRevision;
  const data=await call("getHome",{pageSize:8});
  if(revision!==renderRevision)return null;
  const quick=(data.quick||[]).slice(0,6); const rec=(data.recent||[]).slice(0,8);
  const recentAlbums=[...new Map(rec.filter(track=>track.album).map(track=>[`${track.album}\0${track.albumArtist||track.artist}`,{...track,id:track.album,name:track.album,title:track.album,artist:track.albumArtist||track.artist,album:track.album}])).values()].slice(0,5);
  const playlistData=await call("getPlaylists"); if(revision!==renderRevision)return null; let libraryCards=(playlistData.playlists||[]).slice(0,5); let libraryType="playlist",libraryView="Playlists";
  if(!libraryCards.length){const albums=await call("getGroups",{column:"album",offset:0,pageSize:5});if(revision!==renderRevision)return null;libraryCards=albums.groups||[];libraryType="album";libraryView="Albums";}
  state.indexedBytes=Number(data.totalBytes)||0;
  const hour=new Date().getHours(),greeting=hour<12?"Good morning":hour<18?"Good afternoon":"Good evening";
  const subtitle=rec.length?"Pick up where you left off.":data.totalTracks?`${Number(data.totalTracks).toLocaleString()} tracks in your local library`:"Your music library";
  let html=viewHeader(greeting,subtitle);
  html=html.replace('class="page-head"','class="page-head home-head"');
  if(!data.totalTracks) {
    const folderData=await call("getFolders",{rootsOnly:true});if(revision!==renderRevision)return null;const hasRoots=!!folderData.roots?.length,scanning=!!folderData.scan?.active;
    state.homeHasRoots=hasRoots;
    const title=hasRoots?(scanning?"Scanning your music folders":"No indexed tracks yet"):"No music folders added";
    const message=hasRoots?(scanning?"Tracks will appear here as the scan indexes your files.":"Scan your selected folders to add tracks to the library."):"Add a folder to scan your local collection.";
    const action=scanning?"hidden":"";
    return html+`<div class="empty-state" id="homeLibraryEmpty" data-has-roots="${hasRoots}"><b id="homeEmptyTitle">${title}</b><span id="homeEmptyMessage">${message}</span><button class="action primary empty-action" id="homeEmptyAction" data-action="${hasRoots?"scan":"add-folder"}" ${action}>${hasRoots?"Scan folders":"Add music folder"}</button></div>`;
  }
  html+=section("Quick access",quick,"track","Songs");
  if(recentAlbums.length) html+=`<section class="section"><div class="section-head"><h2>Recently played</h2><button data-view="Recently Played">See all</button></div>${cardGrid(recentAlbums,"album")}</section>`;
  if(libraryCards.length) html+=`<section class="section"><div class="section-head"><h2>Made from your library</h2><button data-view="${libraryView}">View all</button></div>${cardGrid(libraryCards,libraryType)}</section>`;
  else if(rec.length) html+=`<section class="section"><div class="section-head"><h2>Recently played</h2><button data-view="Recently Played">See all</button></div>${cardGrid(rec.slice(0,5),"track")}</section>`;
  return html;
}

async function renderSearch() {
  const requestRevision=++searchRevision,requestId=nextSearchRequestId();
  const query=state.search.trim(),filter=state.filter;let html=viewHeader("Search",query?`Results in your local library for “${query}”`:"Find songs, artists, albums, and playlists.");
  await call("beginSearch",{requestId},false);
  if(requestRevision!==searchRevision||query!==state.search.trim()||filter!==state.filter||state.view!=="Search")return null;
  if(!query) return html+`<div class="empty-state"><b>Search your library</b>Type in the single search field above to get started.</div>`;
  if(state.searchQuery!==query){state.searchQuery=query;state.searchPages={songs:0,albums:0,artists:0,playlists:0};state.searchResults={};}
  const definitions={songs:{filter:"song",label:"Songs",kind:"track",pageSize:40},albums:{filter:"album",label:"Albums",kind:"album",pageSize:24},artists:{filter:"artist",label:"Artists",kind:"artist",pageSize:24},playlists:{filter:"playlist",label:"Playlists",kind:"playlist",pageSize:20}};
  const selected=filter==="all"?Object.keys(definitions):[{song:"songs",album:"albums",artist:"artists",playlist:"playlists"}[filter]].filter(Boolean);
  const pages=await Promise.all(selected.map(async key=>{const def=definitions[key];let page=state.searchPages[key]||0;try{let result=await call("search",{query,filter:def.filter,offset:page*def.pageSize,pageSize:def.pageSize,requestId},false);if(!result||!Array.isArray(result.items))throw new Error("The library returned an invalid search response.");const lastPage=Math.max(0,Math.ceil((result.totalCount||0)/def.pageSize)-1);if(page>lastPage){page=lastPage;result=await call("search",{query,filter:def.filter,offset:page*def.pageSize,pageSize:def.pageSize,requestId},false);if(!result||!Array.isArray(result.items))throw new Error("The library returned an invalid search response.");}return [key,result,page];}catch(error){return [key,{error:error.message||"Search failed."}];}}));
  if(requestRevision!==searchRevision||query!==state.search.trim()||filter!==state.filter||state.view!=="Search")return null;
  for(const [key,data,page] of pages)if(!data.error){state.searchResults[key]=data;state.searchPages[key]=page;}
  html+=`<div class="pill-row">${["all","song","album","artist","playlist"].map(f=>`<button class="chip ${filter===f?"active":""}" data-filter="${f}">${f==="all"?"All":`${f[0].toUpperCase()}${f.slice(1)}s`}</button>`).join("")}</div>`;
  let any=false;
  const failures=[];
  for(const [key,data] of pages)if(data.error)failures.push(key);
  if(failures.length) {
    const label=failures.map(key=>definitions[key].label).join(", ");
    html+=`<div class="empty-state search-error"><b>Could not search ${esc(label)}</b>The library query failed. Check the local log and try again.<button class="action" data-action="retry-search">Try again</button></div>`;
    any=true;
    toast("Search could not be completed. Check the local log for details.");
  }
  for(const key of selected){const def=definitions[key],fresh=pages.find(entry=>entry[0]===key)?.[1],data=fresh?.error?{items:[],totalCount:0}:state.searchResults[key]||{items:[],totalCount:0},items=data.items||[],total=data.totalCount||0,page=state.searchPages[key]||0,totalPages=Math.max(1,Math.ceil(total/def.pageSize));if(!items.length)continue;any=true;
    html+=`<section class="section"><div class="section-head"><h2>${def.label}</h2><span class="muted">${Number(total).toLocaleString()}</span></div>${key==="songs"?songTable(items,page*def.pageSize):cardGrid(items,def.kind)}${totalPages>1?`<div class="pagination"><button class="action" data-action="search-page" data-type="${key}" data-direction="-1" ${page===0?"disabled":""}>Previous</button><span class="muted">Page ${page+1} of ${totalPages}</span><button class="action" data-action="search-page" data-type="${key}" data-direction="1" ${page+1>=totalPages?"disabled":""}>Next</button></div>`:""}</section>`;
  }
  if(!any)html+=`<div class="empty-state"><b>No results for “${esc(query)}”</b>Try another song, artist, album, or playlist name.</div>`;
  return html;
}

function viewFilter(view) { return ({"Favorites":"favorites","Most Played":"most-played","Recently Played":"recent","Recently Added":"recently-added","With Lyrics":"with-lyrics","Videos":"videos"})[view]||null; }
async function renderTracksView(append=false) {
  const revision=renderRevision;
  const page=await call("getTracks",{view:state.view,search:state.search,sort:state.sort,descending:state.descending,offset:state.offset,pageSize:state.pageSize});
  if(revision!==renderRevision)return null;
  const pageTracks=page.tracks||[];const previousCount=append?state.items.length:0;
  state.total=page.totalCount||0; state.items=append?[...state.items,...pageTracks]:pageTracks;state.nextOffset=state.offset+pageTracks.length;
  if(append&&!pageTracks.length)return "";
  const subtitle=`${Number(state.total).toLocaleString()} ${state.view==="Videos"?"videos":"tracks"}`;
  const sorts=["Title","Artist","Album","Genre","Year","Added","Duration","PlayCount","LastPlayed","Path","Rating",...(state.videoSupportEnabled?["Type"]:[])];
  let html=append?"":viewHeader(state.view,subtitle,`<button class="action" data-action="play-view">Play all</button><button class="action" data-action="shuffle-view">Shuffle</button>`)+toolbar(`<label class="muted">Sort</label><select class="select" id="sortSelect">${sorts.map(x=>`<option ${state.sort===x?"selected":""}>${x}</option>`).join("")}</select><button class="action" data-action="direction">${state.descending?"Descending":"Ascending"}</button>`);
  html+=songTable(append?pageTracks:state.items,previousCount);
  if(state.nextOffset<state.total&&pageTracks.length) html+=`<button class="action load-more" data-action="load-more">Load more tracks</button>`;
  return html;
}

async function renderGroupsView(append=false) {
  const revision=renderRevision;
  const column=({Albums:"album",Artists:"artist",Genres:"genre"})[state.view];
  const data=await call("getGroups",{column,offset:state.offset,pageSize:60,search:state.search});
  if(revision!==renderRevision)return null;
  const pageGroups=(data.groups||[]).filter(item=>{
    const label=String(item?.name??item?.title??"").trim();
    return !!label&&!/^(?:undefined|null)$/i.test(label);
  });state.total=data.totalCount||0; state.items=append?[...state.items,...pageGroups]:pageGroups;state.nextOffset=state.offset+(data.groups||[]).length;
  if(append&&!pageGroups.length)return state.nextOffset<state.total&&(data.groups||[]).length?`<button class="action load-more" data-action="load-more">Load more</button>`:"";
  const html=(append?"":viewHeader(state.view,`${Number(state.total).toLocaleString()} ${state.view.toLowerCase()}`))+cardGrid(append?pageGroups:state.items,column)+ (state.nextOffset<state.total&&(data.groups||[]).length?`<button class="action load-more" data-action="load-more">Load more</button>`:"");
  return html;
}

async function renderFolders(append=false) {
  const revision=renderRevision;const offset=state.offset;const data=await call("getFolders",{offset,pageSize:50});if(revision!==renderRevision)return null;const roots=data.roots||[];const status=data.scan||null;
  const pageFolders=data.folders||[];state.folderItems=append?[...(state.folderItems||[]),...pageFolders]:pageFolders;state.nextOffset=offset+pageFolders.length;state.folderOffset=data.folderOffset||0;state.folderCount=data.folderCount||0;
  if(append&&!pageFolders.length)return "";
  if(append)return cardGrid(pageFolders,"folder")+(state.nextOffset<state.folderCount&&pageFolders.length?`<button class="action load-more" data-action="load-folders">Load more folders</button>`:"");
  return viewHeader("Music folders","Choose where Music Player should look for supported media.",`<button class="action primary" data-action="add-folder">＋ Add folder</button><button class="action" data-action="scan">Scan now</button><button class="action" data-action="rebuild-index" title="Read file metadata again and reconcile missing tracks">Rebuild index</button><button class="action" data-action="manage-roots">Manage folders</button><button class="action" data-action="manage-exclusions">Scan exclusions</button>`)
    +`<div class="folder-row scan-status" id="folderScanStatus" ${status?.active?"":"hidden"}><span class="scan-dot"></span><div><b id="folderScanTitle">${status?.paused?"Scan paused":"Scanning library"}</b><span id="folderScanDetails">${esc(status?.currentPath||"")} · ${status?.filesFound||0} tracks · ${status?.directoriesVisited||0} folders</span></div></div>`
    +`<section class="section"><div class="section-head"><h2>Indexed folders</h2><span class="muted">${Number(state.folderCount).toLocaleString()}</span></div>${cardGrid(state.folderItems,"folder")}</section>`
    +(state.nextOffset<state.folderCount&&pageFolders.length?`<button class="action load-more" data-action="load-folders">Load more folders</button>`:"")
    +`<section class="section"><div class="section-head"><h2>Library roots</h2><button data-action="manage-roots">Manage</button></div><div class="folder-grid">${roots.map(root=>{const available=root.available!==false&&root.status!=="Unavailable";return `<div class="folder-row ${available?"":"folder-unavailable"}"><div class="folder-icon"></div><div style="min-width:0;flex:1"><b title="${esc(root.path)}">${esc(root.name||root.path)}</b><span>${esc(root.path)} · ${Number(root.trackCount||0).toLocaleString()} tracks · ${available?"Available":"Unavailable"}</span>${available?"":`<small class="folder-warning">Reconnect this drive, then rescan the library.</small>`}</div><button class="action" data-action="remove-root" data-path="${esc(root.path)}">Remove</button></div>`;}).join("")}</div>${roots.length?"":`<div class="empty-state"><b>No music folders added</b>Add a folder to build your local library.</div>`}</section>`;
}

async function renderPlaylists() {
  const revision=renderRevision;const data=await call("getPlaylists");if(revision!==renderRevision)return null; const lists=data.playlists||[];
  const smart=[["Most Played","trending","Songs played most often"],["With Lyrics","lyrics","Songs with available lyrics"],["Recently Played","clock","Your recent listening history"]];
  if(data.videosEnabled)smart.push(["Videos","film","Video files only"]);
  const smartRows=smart.map(([view,icon,description])=>`<button class="smart-playlist" data-view="${esc(view)}"><span class="smart-playlist-icon" data-icon="${icon}"></span><span><b>${esc(view)}</b><small>${esc(description)}</small></span><span class="smart-playlist-open">›</span></button>`).join("");
  return viewHeader("Playlists","Your playlists are stored on this PC.",`<button class="action" data-action="import-playlist">Import M3U / M3U8</button><button class="action primary" data-action="new-playlist">＋ New playlist</button>`)+`<section class="section smart-playlists"><div class="section-head"><h2>Smart playlists</h2></div><div class="smart-playlist-grid">${smartRows}</div></section><section class="section"><div class="section-head"><h2>Your playlists</h2></div>${cardGrid(lists,"playlist")}</section>`;
}

async function renderPlaylistDetail(append=false) {
  if(!state.playlist) return renderPlaylists();
  const revision=renderRevision;const playlist=state.playlist;
  const page=await call("getPlaylistTracks",{playlistId:playlist.id,search:state.search,offset:state.offset,pageSize:state.pageSize});if(revision!==renderRevision)return null;
  const pageTracks=page.tracks||[],previousCount=append?state.items.length:0;state.total=page.totalCount||0;state.items=append?[...state.items,...pageTracks]:pageTracks;state.nextOffset=state.offset+pageTracks.length;
  if(append&&!pageTracks.length)return "";
  const listing=pageTracks.length?songTable(pageTracks,previousCount):`<div class="empty-state"><b>${state.search?"No tracks match this search":"This playlist is empty"}</b>${state.search?"Try another title, artist, or album.":"Add tracks from your library to get started."}</div>`;
  const pageContent=listing+(state.nextOffset<state.total&&pageTracks.length?`<button class="action load-more" data-action="load-more">Load more tracks</button>`:"");
  return append?pageContent:viewHeader(state.playlist.name,`${Number(state.total).toLocaleString()} ${state.search?"matching ":""}tracks`,toolbar(`<button class="action primary" data-action="play-playlist">Play</button><button class="action" data-action="shuffle-playlist">Shuffle</button><button class="action" data-action="rename-playlist">Rename</button><button class="action" data-action="export-playlist">Export</button><button class="action danger" data-action="delete-playlist">Delete</button>`))+pageContent;
}

async function renderGroupDetail(append=false) {
  const revision=renderRevision;const group=state.group; if(!group||!group.name||/^(?:undefined|null)$/i.test(String(group.name))) { await navigate(state.view==="Album"?"Albums":state.view==="Artist"?"Artists":state.view==="Folder"?"Folders":"Genres",false); return null; }
  const detailSort=group.column==="artist"?"PlayCount":group.column==="album"?"TrackNumber":state.sort;
  const detailDescending=group.column==="artist"?true:group.column==="album"?false:state.descending;
  const page=await call("getTracks",{view:state.view,groupColumn:group.column,groupValue:group.name,offset:state.offset,pageSize:state.pageSize,sort:detailSort,descending:detailDescending});
  if(revision!==renderRevision)return null;
  const pageTracks=page.tracks||[],previousCount=append?state.items.length:0;state.total=page.totalCount||0;state.items=append?[...state.items,...pageTracks]:pageTracks;state.nextOffset=state.offset+pageTracks.length;
  if(append&&!pageTracks.length)return "";
  const sample=state.items[0]||group;let albums=[];if(group.column==="artist"&&!append){const a=await call("getArtistAlbums",{artist:group.name,offset:0,pageSize:30});if(revision!==renderRevision)return null;albums=a.groups||[];state.artistAlbumItems=albums;state.artistAlbumsOffset=albums.length;state.artistAlbumsHaveMore=a.hasMore===true;}
  const tracksTitle=group.column==="artist"?"Popular tracks":"Tracks";
  if(append)return songTable(pageTracks,previousCount)+(state.nextOffset<state.total&&pageTracks.length?`<button class="action load-more" data-action="load-more">Load more tracks</button>`:"");
  const albumSection=albums.length?`<section class="section"><div class="section-head"><h2>Albums</h2></div><div class="card-grid" id="artistAlbumsGrid">${albums.map(item=>card(item,"album")).join("")}</div>${state.artistAlbumsHaveMore?`<button class="action load-more" data-action="load-artist-albums">More albums</button>`:""}</section>`:"";
  const tracksMore=state.nextOffset<state.total&&pageTracks.length?`<button class="action load-more" data-action="load-more">Load more tracks</button>`:"";
  return `<div class="detail-hero">${cover({...sample,name:group.name,album:group.column==="album"?group.name:sample.album,artist:group.column==="artist"?group.name:sample.artist},group.column==="artist"?"round":"")}<div><div class="eyebrow">${esc(group.column)}</div><h1>${esc(group.name)}</h1><p>${Number(state.total).toLocaleString()} tracks${sample.year?` · ${sample.year}`:""}</p><div class="toolbar"><button class="action primary" data-action="play-view">Play</button><button class="action" data-action="shuffle-view">Shuffle</button></div></div></div>${albumSection}<section class="section"><div class="section-head"><h2>${tracksTitle}</h2></div>${songTable(state.items)}${tracksMore}</section>`;
}

async function renderQueue(append=false,background=false) {
  const mountedRows=background?$$('.queue-table tr[data-queue-index]'):[];
  if(!append)state.offset=mountedRows.length?Math.min(Number(mountedRows[0].dataset.queueIndex),Math.max(0,state.queueIndex)):Math.max(0,state.queueIndex);
  const revision=renderRevision,queueRevision=state.queueEventRevision||0,start=state.offset,wanted=Math.max(200,mountedRows.length);let entries=[],data;
  do{
    data=await call("getQueue",{offset:start+entries.length,pageSize:Math.min(200,wanted-entries.length)});
    if(revision!==renderRevision)return null;
    const page=data.entries||[];entries.push(...page);if(!page.length)break;
  }while(!append&&entries.length<wanted&&start+entries.length<data.totalCount);
  const previous=append?(state.queueItems||[]):[];
  state.queueItems=entries;state.queuePageOffset=start;state.queueTotal=data.totalCount||0;if(queueRevision===(state.queueEventRevision||0))state.queueIndex=data.queueIndex??-1;state.nextOffset=start+entries.length;
  if(append&&!state.queueItems.length)return "";
  let html=append?"":viewHeader("Queue",`${Math.max(0,state.queueTotal-state.queueIndex-1)} upcoming tracks`,toolbar(`<button class="action" data-action="clear-queue">Clear upcoming</button>`));
  html+=state.queueItems.length?`<div class="table-wrap"><table class="song-table queue-table"><thead><tr><th class="index">#</th><th>Title</th><th class="queue-actions-col">Actions</th></tr></thead><tbody>${state.queueItems.map((t,i)=>{const index=state.queuePageOffset+i;const current=index===state.queueIndex;const subtitle=t.fileUnavailable?(t.artist==="Unavailable"?"File unavailable":`${t.artist} · File unavailable`):t.artist;return `<tr data-queue-index="${index}" data-context="track" data-context-id="${esc(t.id)}" tabindex="0" aria-label="Play ${esc(t.title)}${current?", currently playing":""}" ${current?'aria-current="true"':""} class="queue-track-row ${current?"queue-current":""} ${t.fileUnavailable?"unavailable-track":""}"><td class="index">${current?"▶":index+1}</td><td><div class="song-cell">${cover(t,"song-thumb")}<div class="song-info"><b>${esc(t.title)}</b><span>${esc(subtitle)}</span></div></div></td><td class="queue-actions-col"><div class="queue-actions"><button class="ctrl queue-action" data-action="play-queue" data-index="${index}" title="Play queue entry" aria-label="Play queue entry">▶</button><button class="ctrl queue-action queue-drag-handle" data-queue-drag="${index}" title="Drag to reorder" aria-label="Drag to reorder queue entry" ${index<=state.queueIndex?"disabled":""}>⠿</button><button class="ctrl queue-action" data-action="queue-remove" data-index="${index}" title="Remove" aria-label="Remove from queue">×</button></div></td></tr>`;}).join("")}</tbody></table></div>`:`<div class="empty-state"><b>Queue is empty</b>Songs you add will appear here.</div>`;
  if(state.nextOffset<state.queueTotal&&state.queueItems.length) html+=`<button class="action load-more" data-action="load-more">Load more queued tracks</button>`;
  state.queueItems=[...previous,...entries];
  return html;
}

async function renderNowPlaying() {
  if(!state.track) return viewHeader("Now Playing","Nothing is playing.")+`<div class="empty-state"><b>Choose a song to begin</b>Playback stays active as you move between views.</div>`;
  return `<div class="detail-hero now-detail">${cover(state.track)}<div><div class="eyebrow">Now playing</div><h1>${esc(state.track.title)}</h1><p>${esc(state.track.artist)}${state.track.album?` · ${esc(state.track.album)}`:""}${state.track.fileUnavailable?" · File unavailable":""}</p><div class="toolbar"><button class="action" data-action="favorite" data-id="${esc(state.track.id)}">${state.track.favorite?"♥ Favorited":"♡ Favorite"}</button><button class="action" data-action="show-location" data-id="${esc(state.track.id)}">Show in folder</button><button class="action" data-action="details" data-id="${esc(state.track.id)}">Track details</button></div></div></div>`;
}

async function renderLyrics() {
  const revision=renderRevision;
  const lyricsTrackId=state.trackId||state.track?.id;
  const cached=state.lyricsTrack?.id===lyricsTrackId&&(state.lyricsRaw||state.lyricLines?.length||state.lyricText);
  const loading=state.lyricsLoadingTrackId===lyricsTrackId;
  const data=cached?{track:state.lyricsTrack,lines:state.lyricLines,plainText:state.lyricText,raw:state.lyricsRaw,source:state.lyricsSource}:loading?{track:state.track?.id===lyricsTrackId?state.track:null,lines:[],plainText:"",raw:"",source:"none"}:await ensureLyricsLoaded(lyricsTrackId,!!state.settings.autoLoadLyrics);
  if(revision!==renderRevision)return null; state.lyricsTrack=data.track||null; state.lyricLines=data.lines||[];state.lyricText=data.plainText||"";state.lyricsRaw=data.raw||"";state.lyricsSource=data.source||"none";state.lyricsRevision++;
  if(!state.lyricsTrack) return viewHeader("Lyrics","Choose a song to view lyrics.")+`<div class="empty-state"><b>No track selected</b>Lyrics appear here when a track is playing.</div>`;
  const lyrics=state.lyricLines.length?`<div class="lyrics-lines" id="lyricsLines">${renderLyricLines(state.lyricLines,esc,state.settings.seekFromLyrics!==false&&state.track?.id===lyricsTrackId)}</div>`:state.lyricText?`<p class="lyric-source">Untimed lyrics · automatic following requires timestamps.</p><div class="lyrics-plain">${esc(state.lyricText)}</div>`:`<div class="empty-state"><b>${state.lyricsLoadingTrackId===lyricsTrackId?"Finding lyrics…":"No lyrics found"}</b>${esc(state.lyricsAutoError||"Lyrics saved in the audio file or beside it load automatically. You can search LRCLIB manually.")}</div>`;
  const source=({sidecar:"Sidecar file",embedded:"Embedded in audio tags",lrclib:"LRCLIB (online)",none:"Not saved"})[data.source]||"Source unavailable";
  return viewHeader("Lyrics",`${state.lyricsTrack.title} · ${state.lyricsTrack.artist}`,toolbar(`<button class="action" data-action="edit-lyrics">Edit lyrics</button><button class="action" data-action="search-lyrics" title="Search LRCLIB online for lyrics matching this track">Find lyrics</button>`))+`<div class="lyric-source">Lyrics source · ${esc(source)}${state.settings.seekFromLyrics===false?" · Tap-to-seek is off":state.track?.id===lyricsTrackId?" · Select a timed line to seek":" · Play this track to seek from a lyric line"}</div>${lyrics}`;
}

async function renderAudio() {
  const revision=renderRevision;const data=await call("getAudioSettings");if(revision!==renderRevision)return null;
  const devices=data.devices||[];
  const deviceControl=devices.length
    ? `<select class="select" id="outputDevice">${devices.map(d=>`<option value="${esc(d.id)}" ${d.id===data.selectedId?"selected":""}>${esc(d.name)}</option>`).join("")}</select>`
    : `<span class="muted">No audio output devices are available. Connect a device and refresh.</span>`;
  return viewHeader("Equalizer","Playback, output, and equalizer.")+
    `<section class="setting-section"><h2>Volume</h2><div class="setting-row"><div><b>Output level</b><span>Volume is shared with Windows media controls.</span></div><input id="audioVolume" type="range" min="0" max="100" value="${state.volume}" aria-label="Volume"><button class="action" data-action="toggle-mute">${state.muted?"Unmute":"Mute"}</button></div></section>`+
    `<section class="setting-section"><h2>Audio output</h2><p>Choose a connected Windows playback device.</p><div class="setting-row"><div><b>Output device</b><span>Bluetooth headphones and speakers are supported.</span></div>${deviceControl}<button class="action" data-action="refresh-devices">Refresh</button></div></section>`+
    `<section class="setting-section"><h2>Playback transition</h2><div class="setting-row"><div><b>Crossfade</b><span>Blend into the next track.</span></div><select class="select" id="crossfade">${[0,2,3,5,8,10].map(n=>`<option value="${n}" ${n===data.crossfadeSeconds?"selected":""}>${n?`${n} seconds`:"Off"}</option>`).join("")}</select></div></section>`+
    `<section class="setting-section"><h2>10-band equalizer</h2><div class="setting-row"><div><b>Preset</b><span>Built-in and saved curves.</span></div><select class="select" id="eqPreset">${["Off","Custom",...(data.presets||[])].filter((p,i,all)=>all.indexOf(p)===i).map(p=>`<option ${p===data.currentPreset?"selected ":""}${p==="Custom"?"disabled ":""}value="${esc(p)}">${esc(p)}</option>`).join("")}</select><button class="action" data-action="save-eq">Save custom preset</button></div><div class="eq-bands">${(data.bands||[]).map((b,i)=>`<label><b>${esc(b.label)}</b><input type="range" min="-20" max="20" step="0.5" value="${Number(b.value)||0}" data-eq="${i}" aria-label="${esc(b.label)} gain" aria-orientation="vertical"><span>${Number(b.value||0).toFixed(1)} dB</span></label>`).join("")}</div></section>`;
}

async function renderSettings() {
  const revision=renderRevision;
  const [s,localAbout]=await Promise.all([call("getSettings"),state.aboutInfo?Promise.resolve(state.aboutInfo):call("getAbout").catch(()=>null)]);
  if(revision!==renderRevision)return null;
  state.settings=s;setTheme(s);
  if(localAbout)state.aboutInfo=localAbout;
  const about=state.aboutInfo||{version:state.version||"Unavailable"};
  const indexedBytes=Number(s.indexedLibraryBytes)||0;
  const resolvedTheme=String(s.theme||"System").toLowerCase()==="light"||String(s.theme||"System").toLowerCase()==="system"&&s.resolvedTheme==="Light"?"light":"dark";
  const activeAccent=s.accentManual&&s.accentColor?s.accentColor:s.accentMode==="Artwork"&&s.artworkAccent?s.artworkAccent:resolvedTheme==="light"?"#548c00":"#b7ff2d";
  const row=(key,title,desc,type="toggle",values=[])=>{
    const bounds=key==="navigationWidth"?"min=\"180\" max=\"360\"":key==="browseWidth"?"min=\"180\" max=\"480\"":key==="windowTransparency"?"min=\"0\" max=\"60\" step=\"5\"":"";
    const disabled=key==="selectionColor"&&s.selectionColorMode!=="Custom"?"disabled":"";
    const control=type==="toggle"?`<button class="toggle ${s[key]?"on":""}" data-setting="${key}" role="switch" aria-checked="${!!s[key]}"><i></i></button>`:
      type==="select"?`<select class="select" data-setting="${key}">${values.map(v=>`<option ${String(s[key])===String(v.value)?"selected":""} value="${esc(v.value)}">${esc(v.label)}</option>`).join("")}</select>`:
      type==="number"?`<input class="field setting-number" type="number" ${bounds} step="1" data-setting="${key}" value="${esc(s[key]||"")}">`:
      type==="range"?`<div class="setting-range-control"><input class="setting-range" type="range" ${bounds} data-setting="${key}" value="${esc(s[key]??0)}" ${key==="windowTransparency"&&!s.transparentWindow?"disabled":""}><output>${esc(s[key]??0)}%</output></div>`:
      `<input class="field ${type==="color"?"setting-color":""}" ${type==="color"?`type="color"`:"type=\"text\""} data-setting="${key}" value="${esc(s[key]||"")}" ${disabled}>`;
    return `<div class="setting-row"><div><b>${esc(title)}</b><span>${esc(desc)}</span></div>${control}</div>`;
  };
  const latestRelease=state.latestRelease||state.updateAvailable;
    const componentRows=(about.components||[]).map(item=>`<tr><td><b>${esc(item.name)}</b><small>${esc(item.use)}</small></td><td>${esc(item.version||"See notices")}</td><td>${esc(item.license)}</td><td>${esc(item.source||"See third-party notices")}</td></tr>`).join("");
    const releaseRows=(state.releaseHistory||[]).map(release=>`<button class="about-version" data-action="open-release" data-url="${esc(release.url)}"><b>${esc(release.tag)}</b><span>${release.prerelease?"Preview release":"Stable release"}</span></button>`).join("");
  return viewHeader("Settings","Choose how Music Player looks and behaves.")+`<div class="settings-grid"><nav class="settings-menu" aria-label="Settings sections"><button class="active" data-settings-nav="appearance">Appearance</button><button data-settings-nav="library">Library</button><button data-settings-nav="playback">Playback</button><button data-view="Audio">Equalizer</button><button data-settings-nav="advanced">Advanced</button><button data-settings-nav="about">About</button></nav><div class="settings-panel">`+
    `<section class="setting-section settings-anchor" id="settings-appearance" data-settings-section="appearance"><h2>Appearance</h2><p>Match the player to your desktop.</p>${row("theme","Color theme","System, light, or dark.","select",[{value:"System",label:"System"},{value:"Light",label:"Light"},{value:"Dark",label:"Dark"}])}${row("accentMode","Accent style","Keep the Music Player lime or let album artwork color the whole interface.","select",[{value:"Native",label:"Lime"},{value:"Artwork",label:"Album artwork"}])}${row("accentManual","Use a custom accent","Choose a color for active controls.")}${row("accentColor","Custom accent color","Applied to controls when enabled; artwork still colors the interface.","color")}${row("selectionColorMode","Text selection color","Follow the active theme accent or choose your own.","select",[{value:"Theme",label:"Follow theme"},{value:"Custom",label:"Custom"}])}${row("selectionColor","Custom selection color","Used when text selection is set to Custom.","color")}${row("transparentWindow","Transparent window","Show the desktop behind the player. Off by default.")}${row("windowTransparency","Transparency","0% is opaque. The player always remains at least 40% solid.","range")}${row("motionStyle","Navigation motion","Respect accessibility animation preferences.","select",[{value:"Off",label:"Off"},{value:"Subtle",label:"Subtle"},{value:"Expressive",label:"Expressive"}])}<div class="setting-row"><div><b>Accent presets</b><span>Quick choices for the custom accent.</span></div><div class="accent-list">${[["#b7ff2d","Lime"],["#59d7ff","Sky"],["#a878ff","Violet"],["#ff6e91","Rose"],["#ffb04a","Amber"]].map(([color,label])=>`<button class="accent-dot ${activeAccent===color?"active":""}" data-accent-color="${color}" title="${label}" style="background:${color}"></button>`).join("")}</div></div><div class="toolbar"><button class="action" data-action="reset-ui">Reset UI settings…</button></div></section>`+
    `<section class="setting-section settings-anchor" id="settings-library" data-settings-section="library"><h2>Library</h2><p>Local folders and indexed tracks.</p><div class="setting-row"><div><b>Indexed library</b><span>Total size of tracks currently indexed.</span></div><strong id="indexedLibrarySize">${bytesLabel(indexedBytes)}</strong></div>${row("hideDuplicates","Hide exact duplicates","Hide duplicate rows in track lists. Album, artist, genre, folder, and playlist contents stay complete.")}${["showArtwork","showArtist","showAlbum","showAdded","showYear","showDuration","showFavorite"].map((k,i)=>row(k,["Artwork","Artist","Album","Added date","Year","Duration","Favorite button"][i],"Show this column when the window is wide enough.")).join("")}<div class="toolbar"><button class="action" data-action="manage-roots">Manage folders</button><button class="action" data-action="manage-exclusions">Scan exclusions</button><button class="action" data-action="duplicates">Find duplicates</button><button class="action" data-action="scan">Scan library</button><button class="action" data-action="rebuild-index">Rebuild index and reconcile missing files</button></div><p class="muted">Rebuilding refreshes artwork and removes files confirmed missing from reachable locations. Confirmed deletions are removed from playlists, the queue and saved history. Offline drives are retained and hidden until they reconnect.</p></section>`+
    `<section class="setting-section settings-anchor" id="settings-playback" data-settings-section="playback"><h2>Playback</h2><p>Keep playback state and the queue easy to reach.</p>${row("autoOpenPanel","Open the Now Playing pane when a song starts","Show the current track and upcoming queue.")}${row("autoLoadLyrics","Automatically find lyrics","If no saved lyrics exist, look for a likely LRCLIB match when playback starts. Sends the track title and artist; manual search remains available.")}${row("seekFromLyrics","Seek when selecting a lyric line","Jump playback to a timed lyric. Plain, untimed lyrics cannot seek.")}<div class="toolbar"><button class="action" data-view="Audio">Equalizer and audio output</button></div></section>`+
    `<section class="setting-section settings-anchor" id="settings-advanced" data-settings-section="advanced"><h2>Advanced</h2><p>Application behavior, navigation, and diagnostics.</p>${row("minimizeToTray","Keep running in notification area","Closing the window keeps Music Player and playback running. Use the tray icon to reopen it or choose Quit to exit.")}${row("navigationWidth","Navigation width","Expanded sidebar width in logical pixels.","number")}${row("browseWidth","Browse grid spacing","Spacing between album, artist, and playlist tiles.","number")}<div class="toolbar"><button class="action" id="chooseDefaultPlayer" data-action="open-default-apps" ${s.needsDefaultSelection===false?"hidden":""}>Choose default music player</button><button class="action" data-action="open-logs">Open logs</button><button class="action" data-action="export-logs">Export logs</button></div><h3>Navigation order and visibility</h3><p>Drag the grip or use the arrows to reorder. Use the switch to hide a destination.</p><div class="nav-settings">${navigationSettingsMarkup(s)}</div></section>`+
    `<section class="setting-section settings-anchor" id="settings-about" data-settings-section="about"><h2>About Music Player</h2><p>A local music library and player for Windows.</p><div class="about-identity"><strong>Music Player</strong><span>Version ${esc(about.version||"Unknown")}</span><button class="action about-github" data-action="open-release" data-url="${esc(about.repositoryUrl||"https://github.com/kalabhaftu/music-player")}"><svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 .8a11.2 11.2 0 0 0-3.54 21.83c.56.1.76-.24.76-.54v-2.1c-3.1.68-3.76-1.32-3.76-1.32-.5-1.3-1.24-1.65-1.24-1.65-1.01-.69.08-.68.08-.68 1.12.08 1.7 1.15 1.7 1.15 1 .1.77 2.2 3.55 1.55.1-.72.4-1.2.73-1.48-2.48-.28-5.09-1.24-5.09-5.52 0-1.22.44-2.22 1.15-3-.12-.28-.5-1.42.1-2.95 0 0 .94-.3 3.08 1.14a10.7 10.7 0 0 1 5.6 0c2.14-1.44 3.08-1.14 3.08-1.14.6 1.53.22 2.67.11 2.95.72.78 1.14 1.78 1.14 3.01 0 4.3-2.61 5.23-5.1 5.5.4.35.76 1.02.76 2.06v3.07c0 .3.2.64.77.53A11.2 11.2 0 0 0 12 .8Z"/></svg>View repository</button></div><div class="about-release setting-row"><div><b>Latest release</b><span>${latestRelease?`GitHub release ${esc(latestRelease.tag)} · updates are never installed automatically.`:"Check GitHub to see the latest published version."}</span></div>${latestRelease?`<button class="action" data-action="open-release" data-url="${esc(latestRelease.url)}">View release</button>`:`<button class="action" data-action="open-release" data-url="${esc(about.releasesUrl||"https://github.com/kalabhaftu/music-player/releases")}">Release history</button>`}</div>${row("checkUpdates","Check for updates automatically","Checks GitHub release metadata every six hours; installs are always manual.")}<div class="toolbar"><button class="action" data-action="check-updates" ${state.updateCheckActive?"disabled":""} aria-busy="${state.updateCheckActive}">${state.updateCheckActive?"Checking…":"Check for updates"}</button><button class="action" data-action="open-release" data-url="${esc(about.releasesUrl||"https://github.com/kalabhaftu/music-player/releases")}">View all releases</button></div><h3>Version history</h3><div id="releaseHistory">${releaseRows?`<div class="about-versions">${releaseRows}</div>`:`<p class="muted">Release history is unavailable offline. Use “View all releases” when connected.</p>`}</div><h3>Open-source software</h3><p>Music Player is distributed under the MIT License. Third-party components keep their own licenses and attributions.</p>${componentRows?`<div class="about-table-wrap"><table class="about-table"><thead><tr><th>Component</th><th>Version</th><th>License</th><th>Source</th></tr></thead><tbody>${componentRows}</tbody></table></div>`:`<p class="muted">${about.noticesAvailable?"No third-party components were listed.":"The bundled third-party notices could not be found."}</p>`}<div class="toolbar about-documents"><button class="action" data-action="open-legal" data-file="LICENSE">Open MIT license</button><button class="action" data-action="open-legal" data-file="ThirdPartyNotices.md">Open third-party notices</button><button class="action" data-action="open-legal" data-file="LGPL-2.1.txt">Open LGPL 2.1 text</button></div><h3>Privacy and network use</h3><p>Your music library stays on this device. Manual checks, and automatic checks when enabled, contact GitHub for release information. Searching LRCLIB sends the current track title and artist to LRCLIB to find matching lyrics.</p></section></div></div>`;
}

function navigationSettingsMarkup(settings){const views=$$("#navigation .nav-item[data-view]").map(item=>item.dataset.view);const custom=[...new Set((settings.navigationOrder||[]).filter(view=>view!=="Home"&&views.includes(view)))];const order=["Home",...custom,...views.filter(view=>view!=="Home"&&!custom.includes(view))];const hidden=new Set(settings.hiddenPanels||[]);hidden.delete("Home");return order.map((view,index)=>`<div class="setting-row nav-setting-row" data-nav-row="${esc(view)}">${view==="Home"?`<span class="nav-drag-handle disabled" aria-hidden="true">⋮⋮</span>`:`<button class="nav-drag-handle" type="button" data-nav-drag-handle="${esc(view)}" aria-label="Drag ${esc(view)} to reorder" title="Drag to reorder">⋮⋮</button>`}<b>${esc(view)}</b><div class="toolbar">${view==="Home"?`<span class="muted">Always shown</span>`:`<button class="action" data-nav-move="-1" data-nav-view="${esc(view)}" ${index===1?"disabled":""}>↑</button><button class="action" data-nav-move="1" data-nav-view="${esc(view)}" ${index===order.length-1?"disabled":""}>↓</button><button class="toggle ${hidden.has(view)?"":"on"}" data-nav-visible="${esc(view)}" role="switch" aria-checked="${!hidden.has(view)}"><i></i></button>`}</div></div>`).join("");}

async function renderDuplicates() {
  const revision=renderRevision;const pageSize=100,data=await call("getDuplicates",{offset:state.duplicateOffset,pageSize,search:state.search,sort:state.duplicateSort,descending:state.duplicateDescending});if(revision!==renderRevision)return null;const groups=data.groups||[],total=data.totalCount||0;
  const pages=Math.max(1,Math.ceil(total/pageSize)),page=Math.floor(state.duplicateOffset/pageSize);
  const controls=toolbar(`<label class="muted">Sort</label><select class="select" id="duplicateSort">${["Title","Artist","Album","Year","Added","Duration","PlayCount","LastPlayed","Path","Rating"].map(value=>`<option ${state.duplicateSort===value?"selected":""}>${value}</option>`).join("")}</select><button class="action" data-action="duplicate-direction">${state.duplicateDescending?"Descending":"Ascending"}</button>`);
  const table=groups.length?`<div class="table-wrap"><table class="song-table"><thead><tr><th>Title</th><th>Artist</th><th class="time">Copies</th><th>Locations</th></tr></thead><tbody>${groups.map(g=>`<tr data-duplicate="${esc(g.id)}"><td>${esc(g.title)}</td><td>${esc(g.artist)}</td><td class="time">${g.copyCount}</td><td><button class="action" data-action="duplicate-files" data-id="${esc(g.id)}">Show locations</button></td></tr>`).join("")}</tbody></table></div>`:`<div class="empty-state"><b>${state.search?"No duplicate groups match":"No exact duplicates found"}</b>${state.search?"Try another title, artist, or album.":"Duplicate groups are created as the library is indexed."}</div>`;
  return viewHeader("Duplicates",`${Number(total).toLocaleString()} exact duplicate groups`)+controls+table+(pages>1?`<div class="pagination"><button class="action" data-action="duplicate-page" data-direction="-1" ${page===0?"disabled":""}>Previous</button><span class="muted">Page ${page+1} of ${pages}</span><button class="action" data-action="duplicate-page" data-direction="1" ${page+1>=pages?"disabled":""}>Next</button></div>`:"");
}

async function openDuplicateFiles(id,offset=0) {
  const pageSize=100,data=await call("getDuplicateFiles",{id,offset,pageSize}),files=data.files||[],total=data.totalCount||0,pages=Math.max(1,Math.ceil(total/pageSize)),page=Math.floor(offset/pageSize);
  const body=files.map(file=>`<div class="setting-row"><div><b>${esc(file.title)}</b><span>${esc(file.artist)} · ${esc(file.path)}</span></div><button class="action" data-action="show-file" data-path="${esc(file.path)}">Show in folder</button></div>`).join("")||`<p class="muted">No copies found.</p>`;
  const actions=`${page>0?`<button class="action" data-action="duplicate-files-page" data-id="${esc(id)}" data-offset="${offset-pageSize}">Previous</button>`:""}${page+1<pages?`<button class="action" data-action="duplicate-files-page" data-id="${esc(id)}" data-offset="${offset+pageSize}">Next</button>`:""}<button class="action" data-modal-close>Close</button>`;
  openModal("Duplicate locations",`${body}${pages>1?`<p class="muted">Page ${page+1} of ${pages} · ${Number(total).toLocaleString()} files</p>`:""}`,actions);
}

  return {invalidateCache,capturePage,cacheSize:()=>pages.size,trackRow,songTable,card,cardGrid,section,viewHeader,toolbar,navigate,renderView,loadMore,loadArtistAlbums,renderHome,renderSearch,viewFilter,renderTracksView,renderGroupsView,renderFolders,renderPlaylists,renderPlaylistDetail,renderGroupDetail,renderQueue,renderNowPlaying,renderLyrics,renderAudio,renderSettings,navigationSettingsMarkup,renderDuplicates,openDuplicateFiles};
}
