param([Parameter(Mandatory=$true)][string] $ApplicationId)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Packaged activation testing requires an isolated runner.'}
if(!('MusicPlayerActivation' -as [type])){
Add-Type @'
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMusicPlayerActivation {
  void ActivateApplication([MarshalAs(UnmanagedType.LPWStr)] string appId, [MarshalAs(UnmanagedType.LPWStr)] string args, uint options, out uint processId);
}
public static class MusicPlayerActivation {
  public static uint Start(string appId) {
    var type = Type.GetTypeFromCLSID(new Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C"));
    var manager = (IMusicPlayerActivation)Activator.CreateInstance(type);
    try { uint id; manager.ActivateApplication(appId, "", 0, out id); return id; }
    finally { Marshal.ReleaseComObject(manager); }
  }
}
'@
}
Get-Process -Id ([MusicPlayerActivation]::Start($ApplicationId))
