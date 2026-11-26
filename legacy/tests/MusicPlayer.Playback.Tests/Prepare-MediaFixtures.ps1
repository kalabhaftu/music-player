param([string] $Destination='artifacts/media-fixtures')
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Path $root -Force | Out-Null
if(!(Get-Command ffmpeg -ErrorAction SilentlyContinue)){throw 'FFmpeg is required to generate genuine codec fixtures.'}
foreach($format in @(
    @{ext='wav';codec='pcm_s16le'},@{ext='mp3';codec='libmp3lame'},@{ext='flac';codec='flac'},
    @{ext='aac';codec='aac'},@{ext='m4a';codec='aac'},@{ext='ogg';codec='libvorbis'},
    @{ext='opus';codec='libopus'},@{ext='wma';codec='wmav2'},@{ext='wv';codec='wavpack'},
    @{ext='tta';codec='tta'},@{ext='aiff';codec='pcm_s16be'})) {
    & ffmpeg -hide_banner -loglevel error -y -f lavfi -i 'sine=frequency=440:sample_rate=44100:duration=12' -ac 2 -c:a $format.codec -metadata title='MusicPlayer format fixture' (Join-Path $root "fixture.$($format.ext)")
    if($LASTEXITCODE -ne 0){throw "Fixture encoding failed: $($format.ext)"}
}
# These are real aliases of their respective containers, rather than relabeled codecs.
foreach($alias in @(@{from='wav';to='wave'},@{from='aiff';to='aif'},@{from='m4a';to='m4b'},@{from='ogg';to='oga'})){
    Copy-Item (Join-Path $root "fixture.$($alias.from)") (Join-Path $root "fixture.$($alias.to)") -Force
}
# FFmpeg's official codec corpus supplies formats without an available encoder.
# They are used solely on the test machine, never included in distribution assets.
$sources=@{
    ape='https://samples.ffmpeg.org/A-codecs/lossless/luckynight.ape'
    mpc='https://samples.ffmpeg.org/A-codecs/musepack/03-Take%20A%20Chance%20On%20Me.mpc'
}
foreach($extension in $sources.Keys){
    & curl.exe --fail --location --retry 2 --connect-timeout 15 --max-time 120 --output (Join-Path $root "fixture.$extension") $sources[$extension]
    if($LASTEXITCODE -ne 0){throw "Official fixture download failed: $extension"}
}
# DSD silence containers: field layouts match FFmpeg dsfdec.c and iff.c.
# 0x69 represents balanced one-bit samples; 4096-byte channel blocks for DSF.
$sampleRate=2822400
$samples=$sampleRate*12
$channelBytes=[int]($samples/8)
$dsfBytes=[int]([Math]::Ceiling($channelBytes/4096)*4096*2)
$dsd=[byte[]]::new($dsfBytes); [Array]::Fill[byte]($dsd,0x69)
$writer=[IO.BinaryWriter]::new([IO.File]::Create((Join-Path $root 'fixture.dsf')))
try {
    $writer.Write([Text.Encoding]::ASCII.GetBytes('DSD '));$writer.Write([uint64]28);$writer.Write([uint64](92+$dsfBytes));$writer.Write([uint64]0)
    $writer.Write([Text.Encoding]::ASCII.GetBytes('fmt '));$writer.Write([uint64]52)
    foreach($field in @(1,0,2,2,$sampleRate,1)){$writer.Write([uint32]$field)}
    $writer.Write([uint64]$samples);$writer.Write([uint32]4096);$writer.Write([uint32]0)
    $writer.Write([Text.Encoding]::ASCII.GetBytes('data'));$writer.Write([uint64](12+$dsfBytes));$writer.Write($dsd)
} finally {$writer.Dispose()}
function BigEndian32([uint32] $Value){$bytes=[BitConverter]::GetBytes($Value);[Array]::Reverse($bytes);return ,$bytes}
function Chunk([string] $Id,[byte[]] $Content){
    $bytes=[BitConverter]::GetBytes([uint64]$Content.Length);[Array]::Reverse($bytes)
    return ,([Text.Encoding]::ASCII.GetBytes($Id)+$bytes+$Content+$(if($Content.Length%2){[byte[]]@(0)}else{[byte[]]@()}))
}
$version=Chunk 'FVER' (BigEndian32 0x01050000)
$properties=[Text.Encoding]::ASCII.GetBytes('SND ')+(Chunk 'FS  ' (BigEndian32 $sampleRate))+(Chunk 'CHNL' ([byte[]]@(0,2)+[Text.Encoding]::ASCII.GetBytes('SLFTSRGT')))+(Chunk 'CMPR' ([Text.Encoding]::ASCII.GetBytes('DSD ')+[byte[]]@(0)))
$dsd=[byte[]]::new($channelBytes*2);[Array]::Fill[byte]($dsd,0x69)
$body=[Text.Encoding]::ASCII.GetBytes('DSD ')+$version+(Chunk 'PROP' $properties)+(Chunk 'DSD ' $dsd)
[IO.File]::WriteAllBytes((Join-Path $root 'fixture.dff'),(Chunk 'FRM8' $body))
foreach($fixture in Get-ChildItem $root -Filter 'fixture.*'){
    & ffprobe -v error -show_entries stream=codec_name,sample_rate,channels -show_entries format=duration -of json $fixture.FullName | Set-Content (Join-Path $root "$($fixture.Name).probe.json")
    if($LASTEXITCODE -ne 0){throw "Fixture container/codec validation failed: $($fixture.Name)"}
}
[pscustomobject]@{generator='FFmpeg encoder and DSD silence';downloadSources=$sources} | ConvertTo-Json | Set-Content (Join-Path $root 'sources.json')
# A genuine video with an embedded subtitle stream exercises the same shipped
# video/codec modules as the opted-in native video window.
$subtitle=Join-Path $root 'fixture.srt'
"1`n00:00:00,000 --> 00:00:11,000`nMusic Player subtitle smoke" | Set-Content $subtitle
& ffmpeg -hide_banner -loglevel error -y -f lavfi -i 'testsrc2=size=640x360:rate=24:duration=12' -f lavfi -i 'sine=frequency=440:duration=12' -i $subtitle -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -c:s srt -metadata:s:s:0 title='Release subtitle' -metadata:s:s:0 language=eng (Join-Path $root 'fixture.mkv')
if($LASTEXITCODE -ne 0){throw 'Video/subtitle fixture generation failed.'}
