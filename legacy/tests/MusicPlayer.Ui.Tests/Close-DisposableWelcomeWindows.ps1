param([int] $AppProcessId,[long] $ObservedWindowHandle)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Welcome-window cleanup requires a disposable runner.'}
$players=@(Get-Process MusicPlayer -ErrorAction SilentlyContinue)
if($AppProcessId){
    if($env:MUSICPLAYER_TEST_APP_PID -ne [string]$AppProcessId -or $players.Count -ne 1 -or
        $players[0].Id -ne $AppProcessId -or $players[0].SessionId -ne (Get-Process -Id $PID).SessionId){
        throw 'Welcome-window cleanup may only accompany the sole player launched by this disposable smoke test.'
    }
} elseif($players.Count){throw 'Prelaunch welcome-window cleanup requires no running player.'}
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class DisposableWelcomeWindow {
    delegate bool Callback(IntPtr window,IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumDesktopWindows(IntPtr desktop,Callback callback,IntPtr data);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window,StringBuilder title,int count);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint id);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window,uint message,IntPtr wParam,IntPtr lParam);
    public static IntPtr[] Find(long observedWindow) {
        var result=new HashSet<IntPtr>();
        Action<IntPtr> consider=window=>{
            var title=new StringBuilder(256);GetWindowText(window,title,title.Capacity);
            if(IsWindowVisible(window)&&title.ToString()=="Microsoft account")result.Add(window);
        };
        // EnumWindows omits packaged Windows UI. Include the HWND observed at
        // the blocked input location, still requiring the exact welcome title.
        consider(new IntPtr(observedWindow));
        consider(GetForegroundWindow());
        EnumDesktopWindows(IntPtr.Zero,(window,data)=>{
            consider(window);
            return true;
        },IntPtr.Zero);
        var handles=new IntPtr[result.Count];result.CopyTo(handles);return handles;
    }
}
'@
for($attempt=0;$attempt -lt 12;$attempt++){
    $visible=@([DisposableWelcomeWindow]::Find($ObservedWindowHandle))
    if($visible.Count -eq 0){return}
    foreach($handle in $visible){
        $ownerId=[uint32]0
        $null=[DisposableWelcomeWindow]::GetWindowThreadProcessId($handle,[ref]$ownerId)
        $owner=Get-Process -Id $ownerId
        if($owner.SessionId -ne (Get-Process -Id $PID).SessionId -or !$owner.Path -or
            !$owner.Path.StartsWith($env:WINDIR+'\',[StringComparison]::OrdinalIgnoreCase)){
            throw 'The account welcome window is not owned by the disposable Windows system session.'
        }
        [Console]::Error.WriteLine("Closing optional Microsoft-account welcome UI owned by $($owner.ProcessName).")
        $window=[Windows.Automation.AutomationElement]::FromHandle($handle)
        $pattern=$null
        if($window.TryGetCurrentPattern([Windows.Automation.WindowPattern]::Pattern,[ref]$pattern)){$pattern.Close()}
        else {
            $close=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Close')
            $button=$window.FindFirst([Windows.Automation.TreeScope]::Descendants,$close)
            if($button){$button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()}
            elseif(![DisposableWelcomeWindow]::PostMessage($handle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)){throw 'The optional Windows account welcome UI rejected WM_CLOSE.'}
        }
    }
    Start-Sleep -Milliseconds 250
}
throw 'The optional Windows account welcome UI did not close.'
