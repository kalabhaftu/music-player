export function parseLyricsText(value) {
  const raw = String(value || "").replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n");
  const lines = [];
  const timestamp = /\[(\d+):(\d{1,2})(?:[.:](\d+))?\]/g;
  let offsetMilliseconds = 0;
  for (const sourceLine of raw.split("\n")) {
    const offset = sourceLine.trim().match(/^\[offset:([+-]?\d+)\]$/i);
    if (offset) {
      const parsedOffset = Number(offset[1]);
      if (Number.isFinite(parsedOffset)) offsetMilliseconds = parsedOffset;
      continue;
    }
    const matches = [...sourceLine.matchAll(timestamp)];
    if (!matches.length) continue;
    for (let start = 0; start < matches.length;) {
      let end = start;
      while (end + 1 < matches.length) {
        const between = sourceLine.slice(matches[end].index + matches[end][0].length, matches[end + 1].index);
        if (between.trim()) break;
        end++;
      }
      const textStart = matches[end].index + matches[end][0].length;
      const textEnd = end + 1 < matches.length ? matches[end + 1].index : sourceLine.length;
      const text = sourceLine.slice(textStart, textEnd).trim();
      if (text) {
        for (let index = start; index <= end; index++) {
          const match = matches[index];
          const fraction = match[3] || "";
          const seconds = Number(match[1]) * 60 + Number(match[2]) + (fraction ? Number("0." + fraction) : 0);
          if (Number(match[2]) < 60 && Number.isFinite(seconds)) lines.push({ seconds, text });
        }
      }
      start = end + 1;
    }
  }
  // LRC offsets apply to every line even when the metadata appears at the end.
  for (const line of lines) line.seconds += offsetMilliseconds / 1000;
  lines.sort((a, b) => a.seconds - b.seconds);
  const plainText = lines.length ? "" : raw.split("\n").filter(line => !/^\[[a-z][a-z0-9_-]*:[^\]]*\]$/i.test(line.trim())).map(line => line.replace(timestamp, "").trim()).filter(Boolean).join("\n");
  return { lines, plainText, offsetMilliseconds };

}

export function lyricLinesMarkup(lines, escapeHtml, seekEnabled = true) {
  return (lines || []).map((line, index) => {
    const seconds = Number(line.seconds);
    const text = escapeHtml(line.text || "");
    if (seekEnabled && Number.isFinite(seconds) && seconds >= 0)
      return `<button type="button" class="lyric-line" data-lyric-index="${index}" data-lyric-seconds="${seconds}" aria-label="Seek to ${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}">${text}</button>`;
    return `<p class="lyric-line" data-lyric-index="${index}">${text}</p>`;
  }).join("");
}

function normalizedWords(value) {
  return String(value || "").normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase().replace(/&/g, " and ").replace(/[^\p{L}\p{N}]+/gu, " ").trim().split(/\s+/).filter(Boolean);
}
function lyricIdentity(title,artist){
  title=String(title||"").trim();artist=String(artist||"").trim();
  if(!artist||/^unknown(?: artist)?$/i.test(artist)){artist="";const separator=title.indexOf(" - ");if(separator>0){artist=title.slice(0,separator).trim();title=title.slice(separator+3).trim();}}
  else if(title.toLowerCase().startsWith(`${artist.toLowerCase()} - `))title=title.slice(artist.length+3);
  const decoration=/\b(?:remix|reverb(?:ed)?|slowed|sped[\s-]*up|speed[\s-]*up|nightcore|bass[\s-]*boosted|8d|official|lyrics?|audio|music\s+video|visuali[sz]er|hd|hq|\d{2,4}\s*kbps?|\d{2,4}k)\b/i;
  title=title.replace(/\([^)]*\)|\[[^\]]*\]|\{[^}]*\}/g,part=>decoration.test(part)?" ":part)
    .replace(/(?:\s*[-–—|+]\s*|\s+)(?:slowed|reverb(?:ed)?|sped[\s-]*up|speed[\s-]*up|nightcore|remix|official\s+(?:audio|music\s+video|video)|lyrics?\s+video)\b.*$/i,"").replace(/\s+/g," ").replace(/^[\s–—|+-]+|[\s–—|+-]+$/g,"");
  return {title,artist};
}
function timingMatches(track,result){const duration=Number(track?.durationSeconds),candidateDuration=Number(result?.duration);return !(duration>0&&candidateDuration>0&&Math.abs(duration-candidateDuration)>Math.max(8,duration*.05));}

export function lyricMatchScore(track, result) {
  const identity=lyricIdentity(track?.title,track?.artist),otherIdentity=lyricIdentity(result?.trackName,result?.artistName);
  const title=new Set(normalizedWords(identity.title)),candidate=new Set(normalizedWords(otherIdentity.title));
  if(!title.size||!candidate.size)return -1;
  const common=[...title].filter(word=>candidate.has(word)).length;
  const coverage=common/title.size,precision=common/candidate.size;
  // Do not automatically borrow lyrics from a longer/different song title.
  if(coverage<.9||precision<.8)return -1;
  const artist=new Set(normalizedWords(identity.artist)),other=new Set(normalizedWords(otherIdentity.artist));
  const artistCommon=[...artist].filter(word=>other.has(word)).length;
  if(artist.size&&(!other.size||artistCommon/artist.size<.45||(artistCommon/artist.size<.9&&artistCommon/other.size<.8)))return -1;
  if(!timingMatches(track,result)&&!/(?:remix|slowed|reverb|sped[\s-]*up|speed[\s-]*up|nightcore)/i.test(track?.title||""))return -1;
  return (coverage*.78+precision*.22)*.78+(artist.size?artistCommon/artist.size:1)*.22;
}

export function selectLyricsMatch(track, results) {
  const candidates=(results||[]).map(result=>{
    const synced=String(result?.syncedLyrics||"").trim(),plain=String(result?.plainLyrics||"").trim();
    const parsed= parseLyricsText(synced);
    const timed=parsed.lines.length>0&&timingMatches(track,result);
    // A slowed/remixed recording may share words but not the original timestamps.
    const raw=timed?synced:plain||(parsed.lines.length?parsed.lines.map(line=>line.text).join("\n"):"");
    return {result,score:lyricMatchScore(track,result),raw,timed};
  }).filter(item=>item.score>=0&&item.raw);
  candidates.sort((a,b)=>Number(b.timed)-Number(a.timed)||b.score-a.score);
  return candidates[0]||null;
}

export function activeLyricIndex(lines, position) {
  let low=0,high=lines.length;
  while(low<high){const mid=(low+high)>>>1;if(Number(lines[mid].seconds)<=position)low=mid+1;else high=mid;}
  return low-1;
}

export function createLyricsLoader(loadLocal, searchResults, now=Date.now) {
  const cache=new Map(),pending=new Map();
  return {
    forget(id){cache.delete(id);},
    async load(id,{allowRemote=false,track=null}={}) {
      if(!id)return {track:null,raw:"",source:"none",lines:[],plainText:""};
      let local;
      try {local=await loadLocal(id);}
      catch {local={track,raw:"",source:"none",lines:[],plainText:""};}
      if(local?.track?.isVideo)return {...local,lines:[],plainText:"",raw:"",source:"none"};
      const raw=String(local?.raw||""),parsed=parseLyricsText(raw);
      local={...local,raw,lines:local?.lines?.length?local.lines:parsed.lines,plainText:local?.plainText||parsed.plainText};
      const hasLocal=local.lines.length>0||!!local.plainText.trim();
      // An untimed sidecar/embedded lyric must not block looking for timestamps.
      // Online lookup only changes the display; the user's file remains intact.
      if(local.lines.length||!allowRemote)return local;
      const saved=cache.get(id);
      if(saved?.expires>now())return hasLocal&&!saved.value.lines.length?local:saved.value;
      if(pending.has(id))return pending.get(id);
      const request=(async()=>{
        try {
          const results=await searchResults(id);
          const match=selectLyricsMatch(local.track||track,results);
          if(!match){
            if(hasLocal)return local;
            return {...local,autoLookupFailed:true,autoLookupMessage:results.length?"LRCLIB returned results, but none provided usable lyrics matching this track confidently.":"No LRCLIB lyrics were found for this track."};
          }
          if(hasLocal&&!match.timed)return local;
          const lyrics=parseLyricsText(match.raw);
          const value={...local,raw:match.raw,...lyrics,source:"lrclib"};
          cache.set(id,{value,expires:now()+(match.timed?30*60_000:5*60_000)});
          if(cache.size>128)cache.delete(cache.keys().next().value);
          return value;
        } catch(error) {
          if(hasLocal)return local;
          return {...local,autoLookupFailed:true,autoLookupMessage:error?.message||"LRCLIB could not be reached. Check your connection or search manually."};
        }
      })().finally(()=>pending.delete(id));
      pending.set(id,request);
      return request;
    }
  };
}

export function createLyricsSearch(fetchResults, now=Date.now) {
  const cache=new Map(),pending=new Map();
  let retryUntil=0;
  function remember(id,results){const timed=results.some(result=>parseLyricsText(result?.syncedLyrics).lines.length);cache.delete(id);cache.set(id,{results,expires:now()+(timed?30*60_000:results.length?5*60_000:60_000)});if(cache.size>128)cache.delete(cache.keys().next().value);}
  return {
    forget(id){cache.delete(id);},
    async search(id,{manual=false}={}) {
      if(!id)return [];
      if(pending.has(id))return pending.get(id);
      const saved=cache.get(id);
      if(saved&&saved.expires>now()&&(!manual||saved.results.length))return saved.results;
      if(retryUntil>now())throw new Error(`LRCLIB is limiting requests. Try again in ${Math.ceil((retryUntil-now())/1000)} seconds.`);
      const request=Promise.resolve().then(()=>fetchResults(id)).then(data=>{
        if(data?.rateLimited||Number(data?.retryAfterMilliseconds)>0){const delay=Math.max(0,Number(data.retryAfterMilliseconds)||0);retryUntil=now()+delay;throw new Error(`LRCLIB is limiting requests. Try again ${delay?`in ${Math.ceil(delay/1000)} seconds`:"shortly"}.`);}
        const results=Array.isArray(data?.results)?data.results:[];
        remember(id,results);return results;
      }).finally(()=>pending.delete(id));
      pending.set(id,request);return request;
    }
  };
}
