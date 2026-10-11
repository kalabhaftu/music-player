import {chromium} from "playwright-core";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import {fileURLToPath} from "node:url";
import {execFileSync} from "node:child_process";
if(process.env.GITHUB_ACTIONS!=="true")throw new Error("Disposable Windows runner required.");
const output=process.env.MUSICPLAYER_TEST_OUTPUT;
await fs.mkdir(output,{recursive:true});
const started=Number(process.env.MUSICPLAYER_STARTED_MS);
let browser,page;
for(let attempt=0;attempt<120;attempt++){
  try{browser=await chromium.connectOverCDP("http://127.0.0.1:9222");break;}catch{await new Promise(r=>setTimeout(r,500));}
}
assert(browser,"WebView did not start.");
for(let attempt=0;attempt<120;attempt++){
  page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith("https://musicplayer.local/"));
  if(page)break;await new Promise(r=>setTimeout(r,500));
}
assert(page,"Music Player navigation did not complete.");
await page.locator('#routeView tr[data-track]').first().waitFor({state:"visible",timeout:120000});
const usableLibraryMs=Date.now()-started;
const measure=()=>JSON.parse(execFileSync("powershell.exe",["-NoLogo","-NoProfile","-File",path.join(path.dirname(fileURLToPath(import.meta.url)),"Measure-Processes.ps1"),"-AppProcessId",process.env.MUSICPLAYER_TEST_APP_PID],{encoding:"utf8",windowsHide:true}));
const ready=measure();
const call=(name)=>page.evaluate(async name=>(await import("/scripts/api.js")).command(name,{}),name);
await call("cancelScan");
const cancelStarted=performance.now();
for(let attempt=0;attempt<120;attempt++){
  if(!(await call("getBootstrap")).scan.active)break;
  await new Promise(r=>setTimeout(r,500));
}
const scanSettled=!(await call("getBootstrap")).scan.active;
if(process.env.MUSICPLAYER_TEST_REVISION==="after")assert.equal(scanSettled,true,"Candidate scan did not settle before idle measurement");
const navigation=[];
for(let cycle=0;cycle<5;cycle++)for(const view of ["Songs","Albums","Artists","Genres"]){
  const beginning=performance.now();
  await page.locator(`#navigation [data-view="${view}"]`).click();
  if(process.env.MUSICPLAYER_TEST_REVISION==="after"){
    await page.waitForFunction(view=>performance.getEntriesByName("music-player-navigation").at(-1)?.detail?.view===view,view);
    const sample=await page.evaluate(()=>performance.getEntriesByName("music-player-navigation").at(-1).detail);
    navigation.push({...sample,visibleLatencyMs:performance.now()-beginning});
    assert(sample.cacheSize<=6,"Navigation cache exceeded six pages");
  }else{
    await page.locator("#routeView .page-head h1").filter({hasText:new RegExp(`^${view}$`)}).waitFor();
    navigation.push({view,visibleLatencyMs:performance.now()-beginning});
  }
}
const percentile=samples=>{const ordered=[...samples].sort((a,b)=>a-b);return ordered[Math.max(0,Math.ceil(ordered.length*.95)-1)]??null;};
const uncachedP95=percentile(navigation.filter(sample=>sample.cached===false).map(sample=>sample.durationMs));
if(process.env.MUSICPLAYER_TEST_REVISION==="after"&&process.env.MUSICPLAYER_TEST_TRACK_COUNT==="100")assert(uncachedP95<=500,`Small-library navigation p95 ${uncachedP95} ms exceeded 500 ms`);
const afterNavigation=measure();
const settled=measure();
await new Promise(r=>setTimeout(r,10000));
const idle=measure();
await page.screenshot({path:`${output}/library.png`,fullPage:true});
await fs.writeFile(`${output}/measurement.json`,JSON.stringify({usableLibraryMs,navigation,uncachedP95,afterNavigation,ready,settled,idle,scanSettled,scanCancellationMs:performance.now()-cancelStarted-10000,cpuMeasurementContext:scanSettled?"idle":"baseline scan cancellation still pending",idleCpuSeconds:scanSettled?Math.max(0,idle.CpuSeconds-settled.CpuSeconds):null,measuredCpuSeconds:Math.max(0,idle.CpuSeconds-settled.CpuSeconds)},null,2));
await browser.close();
