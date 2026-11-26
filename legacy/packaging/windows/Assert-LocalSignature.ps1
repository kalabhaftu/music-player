param([Parameter(Mandatory = $true)][string] $Path)
$ErrorActionPreference = 'Stop'
$file = (Resolve-Path -LiteralPath $Path).Path
$public = [Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $PSScriptRoot 'MusicPlayer-Signing.cer'))
$signature = Get-AuthenticodeSignature -LiteralPath $file
if (!$signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ne $public.Thumbprint -or !$signature.TimeStamperCertificate) {
    throw "Wrong signer or missing timestamp: $file"
}

# Use Windows' Authenticode provider to distinguish damaged signed bytes from
# the expected untrusted-root result of local self-signing. Never import trust.
if (!('MusicPlayerLocalAuthenticode' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MusicPlayerLocalAuthenticode {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct FileInfo {
        public uint Size;
        [MarshalAs(UnmanagedType.LPWStr)] public string Path;
        public IntPtr File, Subject;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct TrustData {
        public uint Size;
        public IntPtr Policy, Sip;
        public uint Ui, Revocation, Choice;
        public IntPtr File;
        public uint StateAction;
        public IntPtr State, Url;
        public uint Flags, Context;
        public IntPtr SignatureSettings;
    }
    [DllImport("wintrust.dll", ExactSpelling = true)]
    private static extern int WinVerifyTrust(IntPtr window, ref Guid action, ref TrustData data);
    public static uint Verify(string path) {
        var file = new FileInfo { Size = (uint)Marshal.SizeOf<FileInfo>(), Path = path };
        var buffer = Marshal.AllocHGlobal(Marshal.SizeOf<FileInfo>());
        try {
            Marshal.StructureToPtr(file, buffer, false);
            var data = new TrustData {
                Size = (uint)Marshal.SizeOf<TrustData>(), Ui = 2, Choice = 1,
                File = buffer, Flags = 0x1010
            };
            var action = new Guid("00AAC56B-CD44-11D0-8CC2-00C04FC295EE");
            return unchecked((uint)WinVerifyTrust(new IntPtr(-1), ref action, ref data));
        } finally {
            Marshal.DestroyStructure<FileInfo>(buffer);
            Marshal.FreeHGlobal(buffer);
        }
    }
}
'@
}
$result = [MusicPlayerLocalAuthenticode]::Verify($file)
if ($result -ne 0 -and $result -ne 0x800B0109L) {
    throw ('Authenticode integrity verification failed (0x{0:X8}): {1}' -f $result, $file)
}
if ($result -eq 0x800B0109L) {
    Write-Verbose 'Signed bytes and pinned project identity checked; the self-signed publisher is not trusted by Windows.'
}
