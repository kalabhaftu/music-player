import test from "node:test";
import assert from "node:assert/strict";
import { parseLyricsText, selectLyricsMatch, createLyricsSearch, createLyricsLoader, activeLyricIndex } from "../../src/MusicPlayer.App/WebUI/scripts/lyrics.js";
import { contrastingInk, relativeLuminance } from "../../src/MusicPlayer.App/WebUI/scripts/colors.js";
import { createPlayerUi } from "../../src/MusicPlayer.App/WebUI/scripts/player.js";

const track={title:"Song",artist:"Artist",durationSeconds:180};
const result={trackName:"Song",artistName:"Artist",duration:180,plainLyrics:"Plain line"};

test("auto lookup prefers a confident timed match over plain lyrics",()=>{
  const match=selectLyricsMatch(track,[result,{...result,syncedLyrics:"[00:01.00]Timed line"}]);
  assert.equal(match.timed,true);
  assert.equal(parseLyricsText(match.raw).lines[0].seconds,1);
});
test("existing untimed local lyrics can be upgraded for display without changing the file",async()=>{
  const local={track,source:"embedded",raw:"My local words"};
  const loader=createLyricsLoader(async()=>local,async()=>[result,{...result,syncedLyrics:"[00:01]Timed line"}]);
  const loaded=await loader.load("a",{allowRemote:true});
  assert.equal(loaded.source,"lrclib");assert.equal(loaded.lines.length,1);
  assert.equal(local.raw,"My local words");
});
test("local plain lyrics survive offline, plain-only, and mismatched remote results",async()=>{
  for(const search of [async()=>{throw new Error("Offline");},async()=>[result],async()=>[{...result,trackName:"Other",syncedLyrics:"[00:01]Wrong"}]]){
    const loader=createLyricsLoader(async()=>({track,source:"sidecar",raw:"My local words"}),search);
    const loaded=await loader.load("a",{allowRemote:true});
    assert.equal(loaded.source,"sidecar");assert.equal(loaded.plainText,"My local words");assert.equal(loaded.autoLookupFailed,undefined);
  }
});
test("timed local lyrics and disabled automatic lookup avoid network requests",async()=>{
  let calls=0;
  const loader=createLyricsLoader(async()=>({track,source:"sidecar",raw:"[00:01]Local timing"}),async()=>{calls++;return [result];});
  assert.equal((await loader.load("a",{allowRemote:true})).source,"sidecar");
  assert.equal(calls,0);
  const disabled=createLyricsLoader(async()=>({track,source:"embedded",raw:"Local words"}),async()=>{calls++;return [result];});
  assert.equal((await disabled.load("a")).plainText,"Local words");assert.equal(calls,0);
});
test("untimed automatic results expire so newly available timestamps can be found",async()=>{
  let now=0,calls=0;
  const search=createLyricsSearch(async()=>({results:++calls===1?[result]:[{...result,syncedLyrics:"[00:01]Timed"}]}),()=>now);
  const loader=createLyricsLoader(async()=>({track,source:"none",raw:""}),id=>search.search(id),()=>now);
  assert.equal((await loader.load("a",{allowRemote:true})).lines.length,0);
  now=5*60_000+1;
  assert.equal((await loader.load("a",{allowRemote:true})).lines.length,1);assert.equal(calls,2);
});
test("wrong song/version/duration cannot win simply by having timestamps",()=>{
  assert.equal(selectLyricsMatch(track,[{...result,trackName:"Song live remix",syncedLyrics:"[00:01]Other"}]),null);
  assert.equal(selectLyricsMatch(track,[{...result,duration:240,syncedLyrics:"[00:01]Other"}]),null);
  assert.equal(selectLyricsMatch({...track,artist:"James Brown"},[{...result,artistName:"James Smith"}]),null);
});
test("Unicode titles and artists match without collapsing to empty strings",()=>{
  assert.ok(selectLyricsMatch({title:"ሙዚቃ",artist:"አርቲስት"},[{trackName:"ሙዚቃ",artistName:"አርቲስት",plainLyrics:"Words"}]));
  assert.equal(selectLyricsMatch({title:"ሙዚቃ",artist:"አርቲስት"},[{trackName:"ሌላ",artistName:"አርቲስት",plainLyrics:"Words"}]),null);
});
test("LRC offset at the end applies to all repeated timestamps and keeps negative times",()=>{
  const parsed=parseLyricsText("[00:01.00][00:02.00]Line\n[00:04.00]Next\n[offset:-1500]");
  assert.deepEqual(parsed.lines.map(line=>line.seconds),[-.5,.5,2.5]);
  assert.equal(activeLyricIndex(parsed.lines,0),0);
  assert.equal(activeLyricIndex(parsed.lines,1),1);
  assert.equal(activeLyricIndex(parsed.lines,.1),0); // Seek backwards.
});
test("invalid timestamps are untimed and metadata is hidden",()=>{
  const parsed=parseLyricsText("[ar:Artist]\n[00:99]Words\n[offset:200]");
  assert.equal(parsed.lines.length,0);
  assert.equal(parsed.plainText,"Words");
  assert.equal(selectLyricsMatch(track,[{...result,syncedLyrics:"[00:99]Bad"}]).raw,"Plain line");
});
test("normal network failure allows immediate manual retry",async()=>{
  let calls=0;
  const search=createLyricsSearch(async()=>{if(++calls===1)throw new Error("Offline");return {results:[result]};});
  await assert.rejects(search.search("a"),/Offline/);
  assert.equal((await search.search("a",{manual:true})).length,1);
  assert.equal(calls,2);
});
test("automatic and manual callers share a slow pending request",async()=>{
  let resolve,calls=0;
  const search=createLyricsSearch(()=>{calls++;return new Promise(done=>{resolve=done;});});
  const first=search.search("a"),second=search.search("a",{manual:true});
  await Promise.resolve();resolve({results:[result]});
  assert.deepEqual(await first,await second);assert.equal(calls,1);
});
test("manual lookup can retry an empty automatic result immediately",async()=>{
  let calls=0;
  const search=createLyricsSearch(async()=>({results:++calls===1?[]:[result]}));
  assert.deepEqual(await search.search("a"),[]);
  assert.equal((await search.search("a",{manual:true})).length,1);
});
test("only the actual server cooldown blocks new lookups",async()=>{
  let now=0,calls=0;
  const search=createLyricsSearch(async()=>++calls===1?{retryAfterMilliseconds:75000}:{results:[result]},()=>now);
  await assert.rejects(search.search("a"),/75 seconds/);
  now=15000;await assert.rejects(search.search("b"),/60 seconds/);assert.equal(calls,1);
  now=76000;assert.equal((await search.search("b")).length,1);
});
test("text ink uses linear sRGB contrast",()=>{
  assert.equal(contrastingInk("#ff0000"),"#101210");
  assert.equal(contrastingInk("#0000ff"),"#ffffff");
  for(const hex of ["#ff0000","#0000ff","#aabbaa","#777777","#eeeeee"]){
    const a=relativeLuminance(hex),b=relativeLuminance(contrastingInk(hex));
    assert.ok((Math.max(a,b)+.05)/(Math.min(a,b)+.05)>=4.5);
  }
});
test("downloaded timed lyrics actually scroll both views and follow a backwards seek",context=>{
  const previousDocument=globalThis.document;
  globalThis.document={hidden:false,addEventListener(){}};
  context.after(()=>{if(previousDocument===undefined)delete globalThis.document;else globalThis.document=previousDocument;});
  const lines=parseLyricsText("[00:01]First\n[00:03]Second").lines;
  function root(){const nodes=lines.map((_,index)=>({dataset:{lyricIndex:String(index)},classList:{toggle(){}},offsetTop:150+index*100,offsetHeight:30,getBoundingClientRect:()=>({top:150+index*100})}));return {dataset:{},nodes,querySelector:selector=>nodes[Number(selector.match(/\d+/)[0])]};}
  const main=root(),immersive=root(),scrolls=[];
  const scroller={scrollTop:0,clientHeight:100,getBoundingClientRect:()=>({top:0}),scrollTo:value=>scrolls.push(value.top)};
  const state={lyricLines:lines,lyricsTrack:{id:"a"},track:{id:"a"},view:"Lyrics",immersiveLyricsOpen:true,playing:false,position:4,settings:{motionStyle:"Off"}};
  const elements={"#lyricsLines":main,"#immersiveLyricsLines":immersive,"#content":scroller,"#immersiveLyricsScroll":scroller,"#immersivePlayer":{hidden:false}};
  const ui=createPlayerUi({state,$:selector=>elements[selector],$$:(_,element)=>element.nodes});
  ui.updateActiveLyric();assert.deepEqual(scrolls,[215,215]);
  ui.updateActiveLyric();assert.equal(scrolls.length,2); // No redundant scrolling.
  state.position=1;ui.updateActiveLyric();assert.deepEqual(scrolls.slice(2),[115,115]);
});
