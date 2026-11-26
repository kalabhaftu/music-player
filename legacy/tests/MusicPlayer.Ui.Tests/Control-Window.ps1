param([Parameter(Mandatory=$true)][int] $AppProcessId,[Parameter(Mandatory=$true)][ValidateSet('Minimize','Restore','TrayRestore','TaskbarToggle','MediaKey','Inspect')][string] $Action,[long] $MainWindowHandle=0)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Native window tests require an isolated Windows runner.'}
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MusicPlayerWindowTest {
  public delegate bool EnumerateWindow(IntPtr window, IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumerateWindow callback, IntPtr data);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint id);
  [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr window, uint command);
  public static IntPtr FindWindow(int processId) {
    IntPtr found=IntPtr.Zero;
    EnumWindows((window,data)=> {uint id; GetWindowThreadProcessId(window,out id);
      if(id==(uint)processId && GetWindow(window,4)==IntPtr.Zero){found=window;return false;}return true;
    },IntPtr.Zero);
    return found;
  }
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
  [StructLayout(LayoutKind.Explicit, Size=40)] public struct Input {
    [FieldOffset(0)] public uint type; [FieldOffset(8)] public ushort key; [FieldOffset(12)] public uint flags;
  }
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint count, Input[] input, int size);
  public static void MediaKey() {
    var inputs = new Input[] { new Input {type=1,key=0xB3}, new Input {type=1,key=0xB3,flags=2} };
    if(SendInput(2,inputs,40)!=2) throw new InvalidOperationException("Windows rejected media key input.");
  }
}
'@
$process=Get-Process -Id $AppProcessId
if($process.ProcessName -ne 'MusicPlayer'){throw 'The target is not the launched Music Player process.'}
$window=if($MainWindowHandle){[IntPtr]$MainWindowHandle}else{$process.MainWindowHandle}
if($window -eq 0){$window=[MusicPlayerWindowTest]::FindWindow($AppProcessId)}
if($window -eq 0){throw 'Music Player has no native window handle.'}
$ownerId=[uint32]0
$null=[MusicPlayerWindowTest]::GetWindowThreadProcessId($window,[ref]$ownerId)
if($ownerId -ne $AppProcessId){throw 'Saved window handle no longer belongs to the test process.'}
switch($Action){
    Minimize {$null=[MusicPlayerWindowTest]::ShowWindow($window,6)}
    Restore {$null=[MusicPlayerWindowTest]::ShowWindow($window,9);$null=[MusicPlayerWindowTest]::SetForegroundWindow($window)}
    TrayRestore {$null=[MusicPlayerWindowTest]::SendMessage($window,0x8029,[IntPtr]::Zero,[IntPtr]0x400)}
    TaskbarToggle {$null=[MusicPlayerWindowTest]::SendMessage($window,0x111,[IntPtr]0x18000002,[IntPtr]::Zero)}
    MediaKey {$null=[MusicPlayerWindowTest]::SetForegroundWindow($window);[MusicPlayerWindowTest]::MediaKey()}
}
[pscustomobject]@{action=$Action;handle=$window.ToInt64().ToString();visible=[MusicPlayerWindowTest]::IsWindowVisible($window)} | ConvertTo-Json -Compress
