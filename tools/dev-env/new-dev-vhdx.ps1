<#
.SYNOPSIS
  Create (or re-attach) an NTFS/Dev Drive virtual disk stored on an exFAT drive so pnpm/npm symlinks work.

.DESCRIPTION
  exFAT has no symlinks or hard links, so `pnpm install` fails on it (EISDIR). This script keeps the data
  physically on the exFAT drive by creating a dynamically expanding VHDX there, formats it as a Dev Drive
  (ReFS, falls back to NTFS) and assigns a drive letter. With -RegisterLogonTask it also registers an
  elevated logon task that re-attaches the disk (opt-in).
  Must run elevated (Administrator). Works on Windows 11 Home (uses diskpart + Storage cmdlets, no Hyper-V).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools\dev-env\new-dev-vhdx.ps1 -VhdxPath E:\VirtualDisks\dev.vhdx -SizeGB 256
#>
[CmdletBinding()]
param(
  [string]$VhdxPath = 'E:\VirtualDisks\dev.vhdx',
  [int]$SizeGB = 256,
  [string]$Label = 'DEV',
  [string]$LogPath = "$env:TEMP\new-dev-vhdx.log",
  # Opt-in: register a logon task (runs elevated) that re-attaches the VHDX when the drive is present.
  [switch]$RegisterLogonTask
)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path $LogPath -Force | Out-Null
try {
  $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
  if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run this script as Administrator.' }

  $dir = Split-Path $VhdxPath -Parent
  if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }

  if (-not (Test-Path $VhdxPath)) {
    Write-Host "Creating dynamic VHDX $VhdxPath (max $SizeGB GB)"
    $dp = Join-Path $env:TEMP 'new-dev-vhdx.diskpart.txt'
    "create vdisk file=`"$VhdxPath`" maximum=$($SizeGB * 1024) type=expandable" | Set-Content -Path $dp -Encoding ascii
    diskpart /s $dp | Write-Host
    if (-not (Test-Path $VhdxPath)) { throw 'diskpart did not create the VHDX (see output above).' }
  }

  $image = Get-DiskImage -ImagePath $VhdxPath
  if (-not $image.Attached) {
    Write-Host 'Attaching VHDX'
    $image = Mount-DiskImage -ImagePath $VhdxPath -PassThru
  }
  $disk = $image | Get-Disk

  if ($disk.PartitionStyle -eq 'RAW') {
    Write-Host 'Initializing disk (GPT) and creating partition'
    Initialize-Disk -Number $disk.Number -PartitionStyle GPT | Out-Null
    $part = New-Partition -DiskNumber $disk.Number -UseMaximumSize -AssignDriveLetter
    try {
      Format-Volume -Partition $part -DevDrive -NewFileSystemLabel $Label -Confirm:$false | Out-Null
      Write-Host 'Formatted as Dev Drive (ReFS).'
    } catch {
      Write-Warning "Dev Drive format failed ($($_.Exception.Message)); formatting NTFS instead."
      Format-Volume -Partition $part -FileSystem NTFS -NewFileSystemLabel $Label -Confirm:$false | Out-Null
    }
  }

  $volume = Get-Disk -Number $disk.Number | Get-Partition | Get-Volume | Where-Object DriveLetter | Select-Object -First 1
  if (-not $volume) { throw 'Volume has no drive letter.' }
  Write-Host "READY: $($volume.DriveLetter): ($($volume.FileSystem), label $($volume.FileSystemLabel))"

  if (-not $RegisterLogonTask) {
    Write-Host "No logon task registered. After reboot or re-plugging the drive, re-attach with: Mount-DiskImage -ImagePath '$VhdxPath' (elevated)"
    return
  }
  # Re-attach automatically at logon when the external drive is present.
  $taskName = 'Attach dev VHDX'
  $cmd = "if (Test-Path '$VhdxPath') { `$i = Get-DiskImage -ImagePath '$VhdxPath'; if (-not `$i.Attached) { Mount-DiskImage -ImagePath '$VhdxPath' | Out-Null } }"
  $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -WindowStyle Hidden -Command `"$cmd`""
  $trigger = New-ScheduledTaskTrigger -AtLogOn
  $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
  Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -RunLevel Highest -Force | Out-Null
  Write-Host "Logon task '$taskName' registered (re-attach manually after plugging the drive in: Mount-DiskImage -ImagePath '$VhdxPath')."
}
catch {
  Write-Host "ERROR: $($_.Exception.Message)"
  exit 1
}
finally {
  Stop-Transcript | Out-Null
}
