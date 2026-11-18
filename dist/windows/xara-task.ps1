<#
.SYNOPSIS
  Start xara at logon on Windows (the counterpart of the systemd user service and the launchd plist).

.DESCRIPTION
  Registers a per-user scheduled task that runs xara.exe, hidden, when you log on, and restarts it if it
  exits.  A per-user task (not a Windows service) because xara needs your audio devices, which a service
  session cannot see.

  xara reads its options from %APPDATA%\xara\xara.conf (copy dist\xara.conf.example there and edit it),
  keeps web UI settings in %APPDATA%\xara, and this task logs to %APPDATA%\xara\xara.log.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File xara-task.ps1 -Exe C:\xara\xara.exe
  powershell -ExecutionPolicy Bypass -File xara-task.ps1 -Remove
#>
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\..\xara.exe'),
    [string]$Name = 'xara',
    [switch]$Remove,
    [switch]$StartNow
)
$ErrorActionPreference = 'Stop'

if ($Remove) {
    Unregister-ScheduledTask -TaskName $Name -Confirm:$false -ErrorAction SilentlyContinue
    Write-Output "removed scheduled task '$Name'"
    return
}

$Exe = (Resolve-Path $Exe).Path
$dir = Join-Path $env:APPDATA 'xara'
New-Item -ItemType Directory -Force $dir | Out-Null
$conf = Join-Path $dir 'xara.conf'
if (-not (Test-Path $conf)) {
    Write-Warning "$conf does not exist yet: copy dist\xara.conf.example there and set at least call, capture and playback"
}
$log = Join-Path $dir 'xara.log'

# conhost --headless (Windows 10 1903+) runs the console program with no window
$action = New-ScheduledTaskAction -Execute 'conhost.exe' -Argument "--headless `"$Exe`" --log `"$log`"" -WorkingDirectory $dir
$trigger = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
# no time limit; restart a crashed modem every minute, up to 999 times
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartInterval (New-TimeSpan -Minutes 1) `
    -RestartCount 999 -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew -Hidden
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $Name -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null
Write-Output "registered scheduled task '$Name': $Exe at logon, log $log"
if ($StartNow) {
    Start-ScheduledTask -TaskName $Name
    Write-Output "started"
}
