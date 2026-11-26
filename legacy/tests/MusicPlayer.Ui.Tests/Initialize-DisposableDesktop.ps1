$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Desktop initialization is restricted to a disposable GitHub runner.'}
# Official OOBE policy prevents subsequent first-sign-in privacy prompts. ARM
# images can already have that screen open, so complete only the observed OOBE UI.
$policy='HKLM:/SOFTWARE/Policies/Microsoft/Windows/OOBE'
New-Item -Path $policy -Force | Out-Null
New-ItemProperty -Path $policy -Name DisablePrivacyExperience -Value 1 -PropertyType DWord -Force | Out-Null
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$root=[Windows.Automation.AutomationElement]::RootElement
$heading=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Choose privacy settings for your device')
$observed=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$heading)
if(!$observed){return}
Write-Host 'Completing first-login privacy setup in the disposable runner.'
$processId=$observed.Current.ProcessId
$process=Get-Process -Id $processId
if(!$process.Path -or !$process.Path.StartsWith($env:WINDIR+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'The privacy prompt is not owned by a Windows system application.'}
$owned=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty,$processId)
for($page=0;$page -lt 8;$page++){
    $elements=$root.FindAll([Windows.Automation.TreeScope]::Descendants,$owned)
    $names=@($elements | ForEach-Object {$_.Current.Name} | Where-Object {$_})
    $output=Join-Path $PWD 'artifacts/ui-evidence'
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $names | ConvertTo-Json | Set-Content (Join-Path $output "oobe-page-$page.json")
    foreach($element in $elements){
        $pattern=$null
        if($element.Current.IsEnabled -and $element.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$pattern) -and $pattern.Current.ToggleState -eq [Windows.Automation.ToggleState]::On){$pattern.Toggle()}
    }
    $next=@($elements | Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.IsEnabled -and $_.Current.Name -in @('Accept','Next')}) | Select-Object -First 1
    if(!$next){throw 'Windows privacy setup has no observed Next/Accept button.'}
    $next.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    Start-Sleep -Milliseconds 750
    if(!$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$heading)){return}
}
throw 'Disposable Windows privacy setup did not finish.'
