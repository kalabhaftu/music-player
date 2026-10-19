import test from "node:test";
import assert from "node:assert/strict";
import { createLibraryViews } from "../../src/MusicPlayer.App/WebUI/scripts/library.js";

// Capture the real renderer's markup and button replacement without a browser dependency.
function fixture(view,call,extra={}) {
  const root={innerHTML:"",insertAdjacentHTML(_where,html){this.innerHTML+=html;},cloneNode(){return {innerHTML:this.innerHTML,querySelectorAll:()=>[]};}};
  const grid={innerHTML:"",insertAdjacentHTML(_where,html){this.innerHTML+=html;}};
  const state={view,offset:0,pageSize:200,items:[],search:"",sort:"Title",descending:false,queueIndex:-1,settings:{},...extra};
  const alerts=[];
  const noop=()=>{};
  const views=createLibraryViews({state,$:selector=>selector==="#artistAlbumsGrid"?grid:root,$$:()=>[],svg:()=>"",paintIcons:noop,esc:value=>String(value??""),initials:()=>"",cover:()=>"",titleOf:item=>item.name||item.title,subOf:()=>"",fmtDuration:()=>"0:03",bytesLabel:String,call,toast:message=>alerts.push(message),setTheme:noop,applyNavigationSettings:noop,openModal:noop,updatePlayer:noop,updatePanel:noop,ensureLyricsLoaded:noop});
  let serial=0;
  function button(action="load-more") {
    const expression=new RegExp(`<button[^>]*data-action="${action}"[^>]*>[^<]*</button>`);
    const match=root.innerHTML.match(expression);
    assert(match,`Missing ${action} button`);
    const markup=match[0].replace("<button",`<button data-test-id="${++serial}"`);
    root.innerHTML=root.innerHTML.replace(match[0],markup);
    return {disabled:false,setAttribute:noop,removeAttribute:noop,
      remove(){root.innerHTML=root.innerHTML.replace(markup,"");},
      insertAdjacentHTML(where,html){assert.equal(where,"beforebegin");root.innerHTML=root.innerHTML.replace(markup,html+markup);}};
  }
  return {state,root,grid,alerts,views,button};
}
const groups=count=>Array.from({length:count},(_,index)=>({id:`g${index}`,name:`Group ${index}`,trackCount:1}));
const tracks=count=>Array.from({length:count},(_,index)=>({id:`t${index}`,title:`Track ${index}`,artist:"Artist",album:"Album"}));
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};};

for(const view of ["Albums","Artists","Genres"])test(`${view}: every card loads once, including a short final page`,async()=>{
  const all=groups(989),offsets=[];
  const f=fixture(view,async(name,{offset,pageSize})=>{
    assert.equal(name,"getGroups");offsets.push(offset);
    return {groups:all.slice(offset,offset+pageSize),totalCount:all.length};
  });
  assert.equal(await f.views.renderView(),true);
  while(f.root.innerHTML.includes('data-action="load-more"'))await f.views.loadMore(f.button());
  assert.equal(f.state.total,989);
  assert.deepEqual(offsets,Array.from({length:17},(_,index)=>index*60));
  assert.deepEqual(f.state.items,all);
  const rendered=[...f.root.innerHTML.matchAll(/data-open-id="(g\d+)"/g)].map(match=>match[1]);
  assert.deepEqual(rendered,all.map(item=>item.id));
});

test("folders: the final short page has no repeated folders and stays before library roots",async()=>{
  const all=groups(73),offsets=[];
  const f=fixture("Folders",async(_name,{offset,pageSize})=>{
    offsets.push(offset);return {folders:all.slice(offset,offset+pageSize),folderCount:73,folderOffset:offset,roots:[]};
  });
  await f.views.renderView();await f.views.loadMore(f.button("load-folders"));
  assert.deepEqual(offsets,[0,50]);
  const rendered=[...f.root.innerHTML.matchAll(/data-open-id="(g\d+)"/g)].map(match=>match[1]);
  assert.deepEqual(rendered,all.map(item=>item.id));
  assert(f.root.innerHTML.indexOf('data-open-id="g72"')<f.root.innerHTML.indexOf("<h2>Library roots</h2>"));
  assert(!f.root.innerHTML.includes('data-action="load-folders"'));
});

for(const view of ["Songs","Favorites","Most Played","Recently Played","Recently Added","With Lyrics","Videos","Playlist","Album","Artist","Genre","Folder"])test(`${view}: appended tracks retain the first page and continuous row numbers`,async()=>{
  const all=tracks(203),offsets=[];
  const f=fixture(view,async(name,payload)=>{
    if(name==="getArtistAlbums")return {groups:[],hasMore:false};
    offsets.push(payload.offset);return {tracks:all.slice(payload.offset,payload.offset+payload.pageSize),totalCount:all.length};
  },{playlist:{id:"list",name:"List"},group:{column:view.toLowerCase(),name:"Collection"}});
  await f.views.renderView();await f.views.loadMore(f.button());
  assert.deepEqual(offsets,[0,200]);assert.deepEqual(f.state.items,all);
  assert.equal((f.root.innerHTML.match(/<tr data-track=/g)||[]).length,203);
  assert(f.root.innerHTML.includes('<td class="index">203</td>'));
  assert(!f.root.innerHTML.includes('data-action="load-more"'));
});

test("queue: duplicate entry identities and absolute action indexes survive paging",async()=>{
  const all=Array.from({length:403},()=>tracks(1)[0]),offsets=[];
  const f=fixture("Queue",async(_name,{offset,pageSize})=>{
    offsets.push(offset);return {entries:all.slice(offset,offset+pageSize),totalCount:403,queueIndex:201};
  });
  await f.views.renderView();while(f.root.innerHTML.includes('data-action="load-more"'))await f.views.loadMore(f.button());
  assert.deepEqual(offsets,[0,200,400]);
  assert.deepEqual([...f.root.innerHTML.matchAll(/<tr data-queue-index="(\d+)"/g)].map(match=>Number(match[1])),Array.from({length:403},(_,index)=>index));
  assert(f.root.innerHTML.includes('data-action="play-queue" data-index="402"'));
});

test("failed Load more retains its button, reports the failure, and retries the same offset",async()=>{
  const all=groups(65),offsets=[];let fail=true;
  const f=fixture("Albums",async(_name,{offset,pageSize})=>{
    offsets.push(offset);if(offset&&fail){fail=false;throw new Error("Offline");}
    return {groups:all.slice(offset,offset+pageSize),totalCount:all.length};
  });
  await f.views.renderView();const button=f.button();
  assert.equal(await f.views.loadMore(button),false);
  assert.equal(f.state.offset,0);assert.equal(button.disabled,false);assert(f.root.innerHTML.includes('data-action="load-more"'));
  assert(f.alerts[0].includes("Offline"));
  assert.equal(await f.views.loadMore(button),true);
  assert.deepEqual(offsets,[0,60,60]);assert.deepEqual(f.state.items,all);
});

test("rapid double clicks make one request and do not skip a batch",async()=>{
  const pending=deferred(),all=groups(65);let calls=0;
  const f=fixture("Albums",async(_name,{offset,pageSize})=>{
    calls++;return offset?pending.promise:{groups:all.slice(0,pageSize),totalCount:65};
  });
  await f.views.renderView();const button=f.button(),first=f.views.loadMore(button);
  assert.equal(await f.views.loadMore(button),false);assert.equal(calls,2);
  pending.resolve({groups:all.slice(60),totalCount:65});await first;
  assert.deepEqual(f.state.items,all);
});

test("navigation during Load more ignores the old response without changing the new page",async()=>{
  const pending=deferred(),all=groups(65);
  const f=fixture("Albums",async(name,{offset,pageSize})=>name==="getTracks"?{tracks:tracks(2),totalCount:2}:offset?pending.promise:{groups:all.slice(0,pageSize),totalCount:65});
  await f.views.renderView();const loading=f.views.loadMore(f.button());
  f.state.view="Songs";await f.views.renderView();const fresh=f.root.innerHTML;
  pending.resolve({groups:all.slice(60),totalCount:65});await loading;
  assert.equal(f.root.innerHTML,fresh);assert.equal(f.state.offset,0);assert.deepEqual(f.state.items,tracks(2));
});

test("refresh after paging starts from the beginning instead of discarding the first page",async()=>{
  const all=groups(65),offsets=[];
  const f=fixture("Albums",async(_name,{offset,pageSize})=>{offsets.push(offset);return {groups:all.slice(offset,offset+pageSize),totalCount:65};});
  await f.views.renderView();await f.views.loadMore(f.button());f.views.invalidateCache();await f.views.renderView();
  assert.deepEqual(offsets,[0,60,0]);assert.deepEqual(f.state.items,all.slice(0,60));
});

test("an empty page after library removal does not leave an endless Load more button",async()=>{
  const f=fixture("Albums",async(_name,{offset})=>({groups:offset?[]:groups(60),totalCount:65}));
  await f.views.renderView();await f.views.loadMore(f.button());
  assert(!f.root.innerHTML.includes('data-action="load-more"'));
});

test("artist albums use the server's hasMore flag, including an exact final batch",async()=>{
  const all=groups(60),offsets=[];
  const f=fixture("Artist",async(name,payload)=>{
    if(name==="getTracks")return {tracks:tracks(2),totalCount:2};
    offsets.push(payload.offset);return {groups:all.slice(payload.offset,payload.offset+30),hasMore:payload.offset===0};
  },{group:{column:"artist",name:"Artist"}});
  await f.views.renderView();await f.views.loadArtistAlbums(f.button("load-artist-albums"));
  assert.deepEqual(offsets,[0,30]);assert.deepEqual(f.state.artistAlbumItems,all);
  assert.equal(f.state.artistAlbumsHaveMore,false);assert(!f.root.innerHTML.includes('data-action="load-artist-albums"'));
  assert.equal((f.grid.innerHTML.match(/data-open-id=/g)||[]).length,30);
});

test("artist album results cannot append to another artist after navigation",async()=>{
  const pending=deferred();let initial=true;
  const f=fixture("Artist",async name=>name==="getTracks"?{tracks:tracks(1),totalCount:1}:initial?(initial=false,{groups:groups(30),hasMore:true}):pending.promise,{group:{column:"artist",name:"Old"}});
  await f.views.renderView();const loading=f.views.loadArtistAlbums(f.button("load-artist-albums"));
  f.state.group={column:"artist",name:"New"};pending.resolve({groups:groups(1),hasMore:false});await loading;
  assert.equal(f.grid.innerHTML,"");assert.equal(f.state.artistAlbumItems.length,30);
});

test("failed artist album loading can retry without skipping albums",async()=>{
  const all=groups(31),offsets=[];let fail=true;
  const f=fixture("Artist",async(name,payload)=>{
    if(name==="getTracks")return {tracks:tracks(1),totalCount:1};
    offsets.push(payload.offset);if(payload.offset&&fail){fail=false;throw new Error("Temporarily unavailable");}
    return {groups:all.slice(payload.offset,payload.offset+30),hasMore:payload.offset===0};
  },{group:{column:"artist",name:"Artist"}});
  await f.views.renderView();const button=f.button("load-artist-albums");
  await f.views.loadArtistAlbums(button);assert.equal(f.state.artistAlbumsOffset,30);assert.equal(button.disabled,false);
  assert(f.alerts[0].includes("Temporarily unavailable"));
  await f.views.loadArtistAlbums(button);assert.deepEqual(offsets,[0,30,30]);assert.deepEqual(f.state.artistAlbumItems,all);
});

for(const [filter,key,size] of [["song","songs",40],["album","albums",24],["artist","artists",24],["playlist","playlists",20]])test(`search ${key}: shrinking results recovers a valid page with working pagination`,async()=>{
  const offsets=[];
  const f=fixture("Search",async(name,payload)=>{
    if(name==="beginSearch")return {};
    offsets.push(payload.offset);return {items:(filter==="song"?tracks(size+1):groups(size+1)).slice(payload.offset,payload.offset+payload.pageSize),totalCount:size+1};
  },{search:"Song",searchQuery:"Song",filter,searchPages:{[key]:5},searchResults:{}});
  await f.views.renderView();
  assert.deepEqual(offsets,[size*5,size]);assert.equal(f.state.searchPages[key],1);
  assert.equal(f.state.searchResults[key].items.length,1);assert(f.root.innerHTML.includes("Page 2 of 2"));
});


test("cached navigation paints immediately, avoids another query and stays bounded at six contexts",async()=>{
  let requests=0;
  const f=fixture("Albums",async()=>{requests++;return {groups:groups(2),totalCount:2};});
  await f.views.renderView();const first=f.root.innerHTML;
  f.state.view="Artists";await f.views.renderView();
  f.state.view="Albums";const ready=f.views.renderView();
  assert.equal(f.root.innerHTML,first);await ready;assert.equal(requests,2);
  for(let i=0;i<20;i++){f.state.search=String(i);await f.views.renderView();assert(f.views.cacheSize()<=6);}
  f.views.invalidateCache();assert.equal(f.views.cacheSize(),0);
});

test("cache invalidation rejects an old response and changing sort produces a different page",async()=>{
  const pending=deferred();let requests=0;
  const f=fixture("Albums",async()=>{requests++;return requests===1?pending.promise:{groups:groups(1),totalCount:1};});
  const old=f.views.renderView();f.views.invalidateCache();f.state.descending=true;
  await f.views.renderView();pending.resolve({groups:groups(2),totalCount:2});
  assert.equal(await old,false);assert.equal(f.state.items.length,1);assert.equal(requests,2);
});

test("offline Settings returns local version without getHome or release history blocking it",async()=>{
  const f=fixture("Settings",async(name)=>{
    if(name==="getSettings")return {indexedLibraryBytes:123,needsDefaultSelection:false};
    if(name==="getAbout")return {version:"1.0.1",components:[]};
    if(name==="getReleaseHistory")throw new Error("Offline");
    throw new Error("Unexpected query "+name);
  });
  await f.views.renderView();assert(f.root.innerHTML.includes("Version 1.0.1"));
  assert(f.root.innerHTML.includes("123"));assert(f.root.innerHTML.includes('id="chooseDefaultPlayer" data-action="open-default-apps" hidden'));
  await new Promise(resolve=>setImmediate(resolve));assert.equal(f.state.aboutInfo.version,"1.0.1");
});
