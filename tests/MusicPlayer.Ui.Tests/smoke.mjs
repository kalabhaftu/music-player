import { chromium } from "playwright-core";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

if(process.env.GITHUB_ACTIONS!=="true")throw new Error("Run only in an isolated GitHub Windows runner.");
const output=process.env.MUSICPLAYER_TEST_OUTPUT||"artifacts/ui-evidence";
await fs.mkdir(output,{recursive:true});
const started=performance.now();
let browser;
for(let attempt=0;attempt<120;attempt++){
  try{browser=await chromium.connectOverCDP("http://127.0.0.1:9222");break;}catch{await new Promise(resolve=>setTimeout(resolve,500));}
}
assert(browser,"WebView2 CDP connection did not become ready");
let page;
for(let attempt=0;attempt<120;attempt++){
  page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith("https://musicplayer.local/"));
  if(page)break;
  await new Promise(resolve=>setTimeout(resolve,500));
}
assert(page,"Embedded Music Player page was not found");
const errors=[];
page.on("pageerror",error=>errors.push(error.message));
const call=(name,payload={})=>page.evaluate(async({name,payload})=>{
  const api=await import("/scripts/api.js");return api.command(name,payload);
},{name,payload});
function metrics(){return JSON.parse(execFileSync("powershell.exe",["-NoLogo","-NoProfile","-File",path.join(path.dirname(fileURLToPath(import.meta.url)),"Measure-Processes.ps1"),"-AppProcessId",process.env.MUSICPLAYER_TEST_APP_PID],{encoding:"utf8",windowsHide:true}));}
let mainWindowHandle;
function nativeWindow(action){
  const args=["-NoLogo","-NoProfile","-File",path.join(path.dirname(fileURLToPath(import.meta.url)),"Control-Window.ps1"),"-AppProcessId",process.env.MUSICPLAYER_TEST_APP_PID,"-Action",action];
  if(mainWindowHandle)args.push("-MainWindowHandle",mainWindowHandle);
  const result=JSON.parse(execFileSync("powershell.exe",args,{encoding:"utf8",windowsHide:true}));
  mainWindowHandle=result.handle;return result;
}
function videoControl(action,name="",value=0){
  return JSON.parse(execFileSync("powershell.exe",["-NoLogo","-NoProfile","-File",path.join(path.dirname(fileURLToPath(import.meta.url)),"Control-Video.ps1"),"-AppProcessId",process.env.MUSICPLAYER_TEST_APP_PID,"-Action",action,"-Name",name,"-Value",String(value)],{encoding:"utf8",windowsHide:true}));
}
async function navigate(view){
  if(view==="Settings"){await page.locator("#topMenuToggle").click();await page.locator('#topMenu [data-view="Settings"]').click();}
  else if(view==="Queue")await page.locator('.player [data-view="Queue"]').click();
  else await page.locator(`#navigation [data-view="${view}"]`).click();
}
async function until(action,message,timeout=120000){
  const deadline=Date.now()+timeout;
  do{if(await action())return;await new Promise(resolve=>setTimeout(resolve,200));}while(Date.now()<deadline);
  throw new Error(message);
}
async function scrollIntoViewAfterRender(selector){
  let lastError;
  for(let attempt=0;attempt<5;attempt++){
    const target=page.locator(selector);
    await target.waitFor({state:"attached"});
    try{await target.scrollIntoViewIfNeeded();return;}
    catch(error){
      if(!String(error?.message||error).includes("not attached to the DOM"))throw error;
      lastError=error;
      await until(async()=>await page.locator(selector).count()===1,"Queue row did not survive its pending render");
    }
  }
  throw lastError||new Error(`Could not scroll ${selector} into view`);
}
await until(async()=>!(await page.locator("#routeView").innerText()).includes("Loading your library"),"Initial view did not render");
assert(!(await page.locator("#routeView").innerText()).includes("could not connect"));
const usableViewMs=performance.now()-started;
if(process.argv.includes("--restart")){
  const restored=await call("getBootstrap");
  assert.equal(restored.queueTotal,5,"Queue did not survive restart");
  assert.equal(restored.playing,false,"Restart resumed playback unexpectedly");
  assert.equal(restored.settings.autoLoadLyrics,false,"Settings did not survive restart");
}
await until(async()=>{
  const data=await call("getTracks",{view:"Songs",pageSize:200});
  return data.tracks.filter(track=>track.title.startsWith("MusicPlayerSmoke-")).length>=4;
},"Automatic clean-profile drive discovery did not find the four fixtures",240000);
await call("cancelScan");
await until(async()=>!(await call("getBootstrap")).scan.active,"Scan cancellation did not complete");
await call("updateSettings",{settings:{autoLoadLyrics:false,motionStyle:"Off"}});
await call("setVolume",{volume:0});
await call("setExtensionEnabled",{extension:".mkv",enabled:true,kind:"Video"});
let video;
await until(async()=>{
  video=(await call("getTracks",{view:"Songs",pageSize:200})).tracks.find(track=>track.title==="MusicPlayerVideoSmoke");
  return !!video;
},"Enabled video fixture was not discovered",240000);
await call("cancelScan");
await call("playTrack",{id:video.id,view:"Songs"});
await until(async()=>{const state=await call("getCurrentTrack");return state.playing&&state.isVideo&&state.positionSeconds>=1;},"Installed video playback did not advance");
videoControl("Activate");
videoControl("Invoke","Pause");
await until(async()=>!(await call("getCurrentTrack")).playing,"Video pause button failed");
videoControl("Seek","",3);
await until(async()=>Math.abs((await call("getCurrentTrack")).positionSeconds-3)<1,"Video seek slider failed");
videoControl("Speed","1.5×");
videoControl("Subtitle");
videoControl("Invoke","Enter full screen (F11)");
// Fullscreen deliberately hides the transport panel; exercise its documented
// Escape accelerator rather than looking for the collapsed exit button.
videoControl("Escape");
videoControl("Invoke","Play");
await until(async()=>(await call("getCurrentTrack")).playing,"Video play button failed");
videoControl("Invoke","Save a screenshot of the current frame");
videoControl("Close");
await until(async()=>!(await call("getCurrentTrack")).playing,"Closing video did not pause playback");
await page.locator('#navigation [data-view="Songs"]').click();
await until(async()=>await page.locator('#routeView tr[data-track]').count()>=4,"Saved songs did not render");
const fixtureRows=await call("getTracks",{view:"Songs",pageSize:200});
const fixtures=fixtureRows.tracks.filter(track=>track.title.startsWith("MusicPlayerSmoke-")).sort((a,b)=>a.title.localeCompare(b.title));
await page.locator("#globalSearch").fill("MusicPlayerSmoke-A");
await until(async()=> (await call("getBootstrap")).view==="Search","Search navigation failed");
await until(async()=> (await page.locator("#routeView").innerText()).includes("MusicPlayerSmoke-A"),"Search result missing");
await page.locator('#navigation [data-view="Songs"]').click();
await until(async()=>await page.locator('#routeView tr[data-track]').count()>=4,"Songs retained the previous search filter");
await call("playTrack",{id:fixtures[0].id,view:"Songs"});
await until(async()=>(await call("getCurrentTrack")).playing,"Native playback did not start");
await call("playPause");
await call("updateSettings",{settings:{accentMode:"Artwork",accentManual:false,theme:"Dark"}});
await until(async()=>!!(await call("getBootstrap")).settings.artworkPalette,"Native artwork palette did not reach the UI");
const artworkThemes=[];
for(const theme of ["Dark","Light"]){
  await call("updateSettings",{settings:{theme}});
  await until(()=>page.evaluate(theme=>document.documentElement.dataset.artwork==="true"&&document.documentElement.dataset.theme===theme.toLowerCase(),theme),"Artwork colors were not applied");
  const sample=await page.evaluate(()=>{
    const root=getComputedStyle(document.documentElement),title=getComputedStyle(document.querySelector("#nowTitle"));
    return {accent:root.getPropertyValue("--accent").trim(),main:root.getPropertyValue("--main-surface").trim(),title:title.color};
  });
  const channels=value=>value.startsWith("#")?[1,3,5].map(i=>parseInt(value.slice(i,i+2),16)):value.match(/[\d.]+/g).slice(0,3).map(Number);
  const luminance=value=>{const c=channels(value).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return c[0]*.2126+c[1]*.7152+c[2]*.0722;};
  assert(channels(sample.main)[0]>channels(sample.main)[2]*2,"Competing cover hues invented a purple surface");
  assert.deepEqual(channels(sample.title),channels(sample.accent),"Now-playing title did not use the artwork accent");
  assert((luminance(sample.title)+.05)/(luminance(sample.main)+.05)>=4.5,"Artwork title contrast is below 4.5:1");
  artworkThemes.push({theme,...sample});
  await page.screenshot({path:`${output}/artwork-${theme.toLowerCase()}.png`,fullPage:true});
}
await call("updateSettings",{settings:{accentMode:"Native",theme:"System"}});
await until(async()=>!(await call("getCurrentTrack")).playing,"Playback did not pause before queue editing");
await call("clearQueue");
for(const track of [fixtures[1],fixtures[1],fixtures[2],fixtures[3]])await call("addToQueue",{id:track.id});
const queueBefore=await call("getQueue",{pageSize:100});
assert.equal(queueBefore.totalCount,5);
assert.equal(queueBefore.entries[1].id,queueBefore.entries[2].id);
await navigate("Queue");
await until(async()=>await page.locator("[data-queue-drag='1']").count()===1,"Queue did not render");
assert.equal(await page.locator("[data-queue-drag='1']").isEnabled(),true,"Upcoming queue entry cannot be dragged");
await until(async()=>{
  const queue=await call("getQueue",{pageSize:100});
  return queue.totalCount===5&&await page.locator(".queue-table tbody tr").count()===5&&await page.locator("[data-queue-index='4']").count()===1;
},"Queue did not settle before its drag interaction");
await page.waitForTimeout(400);
await scrollIntoViewAfterRender("[data-queue-index='4']");
const pointerEvents=[];
await page.exposeFunction("recordQueuePointer",event=>pointerEvents.push(event));
await page.evaluate(()=>{
  for(const type of ["pointerdown","pointermove","pointerup","pointercancel","lostpointercapture"])
    document.querySelector("#routeView").addEventListener(type,event=>window.recordQueuePointer({type:event.type,x:event.clientX,y:event.clientY,target:event.target.outerHTML?.slice(0,300),hit:document.elementFromPoint(event.clientX,event.clientY)?.outerHTML?.slice(0,300),dragging:document.body.classList.contains("queue-pointer-dragging"),drop:document.querySelector(".queue-drop-after,.queue-drop-before")?.dataset.queueIndex}),true);
});
const grip=await page.locator("[data-queue-drag='1']").boundingBox();
const target=await page.locator("[data-queue-index='4']").boundingBox();
assert(grip&&target);
await page.mouse.move(grip.x+grip.width/2,grip.y+grip.height/2);
await page.mouse.down();
await page.mouse.move(target.x+target.width/2,target.y+target.height*.75,{steps:15});
await page.screenshot({path:`${output}/queue-drag.png`,fullPage:true});
await page.mouse.up();
await fs.writeFile(`${output}/queue-pointer.json`,JSON.stringify({queueBefore,grip,target,pointerEvents,queueAfter:await call("getQueue",{pageSize:100})},null,2));
await until(async()=> (await call("getQueue",{pageSize:100})).entries[4].id===fixtures[1].id,"Pointer queue reorder did not persist");
await page.locator("#immersiveToggle").click();
await page.locator("#immersiveLyricsToggle").click();
await until(async()=>await page.locator("#immersiveLyricsLines").innerText().then(text=>text.includes("Smoke lyric")),"Immersive saved lyrics did not appear");
await page.locator("#immersiveLyricsClose").click();
await page.locator("#immersiveLyricsToggle").click();
assert.equal(await page.locator("#immersiveLyricsToggle").getAttribute("aria-expanded"),"true");
await page.locator("#immersiveExit").click();
const memorySamples=[metrics()];
for(let index=0;index<15;index++){
  for(const view of ["Songs","Queue","Settings","Home"]){
    await navigate(view);
    await until(async()=>!(await page.locator("#routeView").innerText()).includes("Loading your library"),"Navigation did not settle");
  }
  if(index%5===4)memorySamples.push(metrics());
}
// Compare the last two batches after route caches have warmed up. The bound
// tolerates browser housekeeping while rejecting sustained per-navigation growth.
const navigationMemoryGrowth=memorySamples.at(-1).PrivateBytes-memorySamples.at(-2).PrivateBytes;
assert(navigationMemoryGrowth<32*1024*1024,"Combined native/WebView memory grew more than 32 MiB over the final 20 navigations");
// Exercise native controls while the renderer is suspended. Only the disposable
// runner receives window messages and a real Windows media-key input.
await call("updateSettings",{settings:{minimizeToTray:true}});
await call("playQueueEntry",{index:0});
await until(async()=>(await call("getCurrentTrack")).playing,"Playback did not resume before minimize");
await call("seek",{seconds:88});
const beforeMinimize=metrics();
assert.equal(nativeWindow("Minimize").visible,false,"Tray minimize did not hide the native window");
await new Promise(resolve=>setTimeout(resolve,5000));
const minimized=metrics();
nativeWindow("TrayRestore");
await until(()=>nativeWindow("Inspect").visible,"Tray activation did not restore the window",15000);
await until(async()=>(await call("getCurrentTrack")).queueIndex===1,"Native end-of-song advancement stopped while WebView was minimized");
nativeWindow("TaskbarToggle");
await until(async()=>!(await call("getCurrentTrack")).playing,"Taskbar thumbnail pause did not control playback");
nativeWindow("TaskbarToggle");
await until(async()=>(await call("getCurrentTrack")).playing,"Taskbar thumbnail play did not control playback");
nativeWindow("MediaKey");
await until(async()=>!(await call("getCurrentTrack")).playing,"Windows media key did not control native playback");
await new Promise(resolve=>setTimeout(resolve,500));
assert.equal((await call("getCurrentTrack")).playing,false,"Media key toggled playback twice");
await call("playQueueEntry",{index:0});
await until(async()=>{const state=await call("getCurrentTrack");return state.playing&&state.queueIndex===0;},"Final queue selection did not start");
await call("playPause");
await until(async()=>!(await call("getCurrentTrack")).playing,"Final paused state did not settle");
// Let the runner close this explicitly launched process after tray behavior has
// been tested; otherwise its window-close path correctly keeps it in the tray.
await call("updateSettings",{settings:{minimizeToTray:false}});
const idleBefore=metrics();
await new Promise(resolve=>setTimeout(resolve,5000));
const idleAfter=metrics();
await page.screenshot({path:`${output}/final-ui.png`,fullPage:true});
await fs.writeFile(`${output}/results${process.argv.includes("--restart")?"-restart":""}.json`,JSON.stringify({usableViewMs,discoveredFixtures:fixtures.length,artworkThemes,installedVideo:{play:true,pause:true,seek:true,speed:true,embeddedSubtitle:true,fullscreen:true,screenshotCommand:true,closePauses:true},queueDragging:true,duplicateQueue:true,immersiveLyrics:true,repeatedNavigation:60,memorySamples,minimizedPlayback:{beforeMinimize,minimized,nativeAdvancement:true,tray:true,taskbar:true,mediaKey:true},pausedIdle:{before:idleBefore,after:idleAfter},errors},null,2));
assert.deepEqual(errors,[],"WebUI raised JavaScript errors");
console.log("Windows WebView UI smoke passed");
await browser.close();
