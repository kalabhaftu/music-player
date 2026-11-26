$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Notification-area probing requires an isolated runner.'}
& (Join-Path $PSScriptRoot 'Initialize-DisposableDesktop.ps1')
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class NotificationAreaProbe {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct Data {
        public uint cbSize; public IntPtr window; public uint id,flags,message; public IntPtr icon;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=128)] public string tip;
        public uint state,mask;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=256)] public string info;
        public uint version;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=64)] public string title;
        public uint infoFlags; public Guid guid; public IntPtr balloon;
    }
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string name,string title);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr CreateWindowEx(uint extended,string name,string title,uint style,int x,int y,int w,int h,IntPtr parent,IntPtr menu,IntPtr instance,IntPtr data);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr LoadIcon(IntPtr instance,IntPtr id);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr window);
    [DllImport("shell32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool Shell_NotifyIcon(uint message,ref Data data);
    public static string Run() {
        var window=CreateWindowEx(0,"STATIC","Music Player CI notification probe",0x80000000,0,0,1,1,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
        if(window==IntPtr.Zero) throw new InvalidOperationException("Could not create the notification probe window.");
        var data=new Data {cbSize=(uint)Marshal.SizeOf(typeof(Data)),window=window,id=1,flags=7,message=0x8029,icon=LoadIcon(IntPtr.Zero,(IntPtr)32512),tip="Music Player CI probe",info="",title=""};
        try {
            var added=Shell_NotifyIcon(0,ref data);var error=Marshal.GetLastWin32Error();
            if(added) Shell_NotifyIcon(2,ref data);
            return "{\"registered\":"+added.ToString().ToLowerInvariant()+",\"nativeError\":"+error+",\"dataSize\":"+data.cbSize+",\"pointerSize\":"+IntPtr.Size+"}";
        } finally {DestroyWindow(window);}
    }
}
'@
if([NotificationAreaProbe]::FindWindow('Shell_TrayWnd',$null) -eq [IntPtr]::Zero){
    Start-Process explorer.exe -WindowStyle Hidden
    for($attempt=0;$attempt -lt 60 -and [NotificationAreaProbe]::FindWindow('Shell_TrayWnd',$null) -eq [IntPtr]::Zero;$attempt++){Start-Sleep -Milliseconds 500}
}
$output=Join-Path $PWD 'artifacts/ui-evidence'
New-Item -ItemType Directory -Path $output -Force | Out-Null
$probe=[NotificationAreaProbe]::Run()
if(!(ConvertFrom-Json $probe).registered){
    if(Get-Process MusicPlayer -ErrorAction SilentlyContinue){throw 'Shell recovery must precede the disposable player launch.'}
    # A shell window alone does not prove that Explorer's notification area is
    # operational. Restart only this disposable runner's Explorer session.
    $session=(Get-Process -Id $PID).SessionId
    Get-Process explorer -ErrorAction SilentlyContinue | Where-Object SessionId -eq $session | Stop-Process -Force
    Start-Sleep -Milliseconds 500
    Start-Process explorer.exe -WindowStyle Hidden
    for($attempt=0;$attempt -lt 30;$attempt++){
        Start-Sleep -Milliseconds 500
        if([NotificationAreaProbe]::FindWindow('Shell_TrayWnd',$null) -ne [IntPtr]::Zero){
            $probe=[NotificationAreaProbe]::Run()
            if((ConvertFrom-Json $probe).registered){break}
        }
    }
}
$probe | Set-Content (Join-Path $output 'notification-area-probe.json')
Write-Host "Independent Windows notification probe: $probe"
if(!(ConvertFrom-Json $probe).registered){
try {
    Add-Type -AssemblyName System.Drawing
    $bitmap=[Drawing.Bitmap]::new([NotificationAreaProbe]::GetSystemMetrics(0),[NotificationAreaProbe]::GetSystemMetrics(1))
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen(0,0,0,0,$bitmap.Size)
        $bitmap.Save((Join-Path $output 'disposable-desktop.png'),[Drawing.Imaging.ImageFormat]::Png)
    } finally {$graphics.Dispose();$bitmap.Dispose()}
} catch {Write-Warning "Disposable desktop capture unavailable: $($_.Exception.Message)"}
throw 'The disposable Windows desktop cannot register notification icons; UI checks require a working desktop.'
}
