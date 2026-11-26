param([Parameter(Mandatory=$true)][string] $TrackPath,[Parameter(Mandatory=$true)][string] $TagLibPath)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Artwork preparation is restricted to disposable runner fixtures.'}
$expected=Join-Path ([Environment]::GetFolderPath('MyMusic')) 'MusicPlayerSmoke/MusicPlayerSmoke-A.wav'
if([IO.Path]::GetFullPath($TrackPath) -ne [IO.Path]::GetFullPath($expected)){throw 'Unexpected artwork fixture path.'}
Add-Type -AssemblyName System.Drawing
$null=[Reflection.Assembly]::LoadFrom((Resolve-Path -LiteralPath $TagLibPath).Path)
$bitmap=[Drawing.Bitmap]::new(32,32)
$stream=[IO.MemoryStream]::new()
try {
    for($x=0;$x -lt 32;$x++){
        $color=if($x -lt 20){[Drawing.Color]::FromArgb(200,15,15)}else{[Drawing.Color]::FromArgb(15,15,200)}
        for($y=0;$y -lt 32;$y++){$bitmap.SetPixel($x,$y,$color)}
    }
    $bitmap.Save($stream,[Drawing.Imaging.ImageFormat]::Png)
    $media=[TagLib.File]::Create($TrackPath,'taglib/wav',[TagLib.ReadStyle]::Average)
    try {
        $picture=[TagLib.Picture]::new([TagLib.ByteVector]::new($stream.ToArray()))
        $picture.MimeType='image/png'
        $picture.Type=[TagLib.PictureType]::FrontCover
        $media.Tag.Pictures=[TagLib.IPicture[]]@($picture)
        $media.Save()
    } finally {$media.Dispose()}
} finally {$bitmap.Dispose();$stream.Dispose()}
