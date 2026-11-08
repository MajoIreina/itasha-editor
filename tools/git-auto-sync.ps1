param(
  [switch]$Install,
  [switch]$Uninstall,
  [switch]$Watch
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$TaskName = 'ItashaEditor-Git-AutoSync'
$PowerShellExe = Join-Path $PSHOME 'powershell.exe'
$LogRoot = Join-Path $env:LOCALAPPDATA 'ItashaEditor-GitAutoSync'
$LogPath = Join-Path $LogRoot 'sync.log'

function Write-SyncLog([string]$Message) {
  if (-not (Test-Path -LiteralPath $LogRoot)) {
    New-Item -ItemType Directory -Path $LogRoot -Force | Out-Null
  }
  Add-Content -LiteralPath $LogPath -Value ('[{0}] {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message) -Encoding UTF8
}

function Invoke-Git([string[]]$GitArgs) {
  $previousPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = 'Continue'
    $result = & git -C $RepoRoot @GitArgs 2>&1
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }
  if ($exitCode -ne 0) {
    throw ('git {0} failed ({1}): {2}' -f ($GitArgs -join ' '), $exitCode, ($result -join ' '))
  }
  return ,$result
}

function Get-Upstream {
  $result = & git -C $RepoRoot rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>$null
  if ($LASTEXITCODE -ne 0) { return $null }
  return [string]$result
}

function Test-GitOperationInProgress {
  foreach ($marker in @('MERGE_HEAD', 'CHERRY_PICK_HEAD', 'REVERT_HEAD', 'rebase-apply', 'rebase-merge')) {
    $path = (& git -C $RepoRoot rev-parse --git-path $marker 2>$null | Select-Object -First 1)
    if ($LASTEXITCODE -eq 0 -and $path -and (Test-Path -LiteralPath (Join-Path $RepoRoot $path))) {
      return $true
    }
  }
  $unmerged = & git -C $RepoRoot ls-files -u 2>$null
  return [bool]$unmerged
}

function Get-ChangeSignature([string]$Status) {
  $parts = [System.Collections.Generic.List[string]]::new()
  $parts.Add($Status)
  foreach ($line in ($Status -split "`r?`n")) {
    if ($line.Length -lt 4) { continue }
    $relativePath = $line.Substring(3).Trim('"')
    if ($relativePath.Contains(' -> ')) { $relativePath = $relativePath.Substring($relativePath.LastIndexOf(' -> ') + 4) }
    $absolutePath = Join-Path $RepoRoot $relativePath
    if (Test-Path -LiteralPath $absolutePath -PathType Leaf) {
      $item = Get-Item -LiteralPath $absolutePath
      $parts.Add(('{0}|{1}|{2}' -f $relativePath, $item.Length, $item.LastWriteTimeUtc.Ticks))
    }
  }
  return [string]::Join("`n", $parts)
}

function Get-Status {
  $previousPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = 'Continue'
    $result = & git -C $RepoRoot status --porcelain=v1 --untracked-files=all --no-renames 2>&1
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }
  if ($exitCode -ne 0) { throw ('Unable to read Git status: ' + ($result -join ' ')) }
  $statusLines = @($result | Where-Object { [string]$_ -match '^[ MADRCU?!]{2} ' })
  if ($statusLines.Count -eq 0) { return '' }
  return [string]::Join("`n", $statusLines)
}

function Push-IfAhead {
  $upstream = Get-Upstream
  if (-not $upstream) {
    Write-SyncLog 'Push skipped: current branch has no upstream. Configure its Git upstream to enable syncing.'
    return
  }
  $ahead = Invoke-Git @('rev-list', '--count', ('{0}..HEAD' -f $upstream))
  if ([int]($ahead | Select-Object -First 1) -gt 0) {
    Invoke-Git @('push') | Out-Null
    Write-SyncLog ('Pushed commits to {0}.' -f $upstream)
  }
}

if ($Uninstall) {
  Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
  Write-Output ('Removed scheduled task {0}.' -f $TaskName)
  exit 0
}

if ($Install) {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
  $argument = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "{0}" -Watch' -f $PSCommandPath
  $action = New-ScheduledTaskAction -Execute $PowerShellExe -Argument $argument -WorkingDirectory $RepoRoot
  $trigger = New-ScheduledTaskTrigger -AtLogOn -User $identity
  $principal = New-ScheduledTaskPrincipal -UserId $identity -LogonType Interactive -RunLevel Limited
  $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
  Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description 'Debounced local Git commit and push for the Itasha Editor repository.' -Force | Out-Null
  Start-ScheduledTask -TaskName $TaskName
  Write-Output ('Installed and started {0}. Sync runs after 30 seconds without file changes.' -f $TaskName)
  Write-Output ('Log: {0}' -f $LogPath)
  exit 0
}

if (-not $Watch) {
  Write-Output 'Use -Install to enable login startup or -Uninstall to disable it.'
  exit 0
}

$mutex = [System.Threading.Mutex]::new($false, 'Local\ItashaEditorGitAutoSync')
$ownsMutex = $false
try {
  try { $ownsMutex = $mutex.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $ownsMutex = $true }
  if (-not $ownsMutex) { exit 0 }

  Write-SyncLog ('Watcher started for {0}.' -f $RepoRoot)
  $lastSignature = $null
  $stableSince = $null
  $lastPushAttempt = [DateTime]::MinValue
  $lastPushErrorLog = [DateTime]::MinValue
  $lastCheckError = $null
  $lastCheckErrorLog = [DateTime]::MinValue

  while ($true) {
    try {
      $status = Get-Status
      $lastCheckError = $null
      if ($status) {
        $signature = Get-ChangeSignature $status
        if ($signature -ne $lastSignature) {
          $lastSignature = $signature
          $stableSince = Get-Date
        } elseif (((Get-Date) - $stableSince).TotalSeconds -ge 30) {
          if (Test-GitOperationInProgress) {
            Write-SyncLog 'Commit paused while a merge, rebase, cherry-pick, revert, or conflict is in progress.'
            $lastSignature = $null
            $stableSince = $null
          } elseif (-not (Get-Upstream)) {
            Write-SyncLog 'Commit paused: current branch has no upstream.'
            $lastSignature = $null
            $stableSince = $null
          } else {
            Invoke-Git @('add', '-A', '--', '.') | Out-Null
            & git -C $RepoRoot diff --cached --quiet
            $diffExit = $LASTEXITCODE
            if ($diffExit -eq 1) {
              $message = 'chore(sync): auto-sync {0}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
              Invoke-Git @('commit', '-m', $message) | Out-Null
              Write-SyncLog ('Committed stable workspace changes: {0}' -f $message)
            } elseif ($diffExit -gt 1) {
              throw 'Unable to inspect staged changes.'
            }
            $lastSignature = $null
            $stableSince = $null
            $lastPushAttempt = Get-Date
            try {
              Push-IfAhead
            } catch {
              $lastPushErrorLog = Get-Date
              Write-SyncLog ('Push failed; local commit is retained and will be retried: {0}' -f $_.Exception.Message)
            }
          }
        }
      } else {
        $lastSignature = $null
        $stableSince = $null
        if (((Get-Date) - $lastPushAttempt).TotalSeconds -ge 30) {
          $lastPushAttempt = Get-Date
          try {
            Push-IfAhead
          } catch {
            if (((Get-Date) - $lastPushErrorLog).TotalSeconds -ge 60) {
              $lastPushErrorLog = Get-Date
              Write-SyncLog ('Push retry failed; local commit is retained: {0}' -f $_.Exception.Message)
            }
          }
        }
      }
    } catch {
      $errorMessage = $_.Exception.Message
      if ($errorMessage -ne $lastCheckError -or ((Get-Date) - $lastCheckErrorLog).TotalSeconds -ge 60) {
        $lastCheckError = $errorMessage
        $lastCheckErrorLog = Get-Date
        Write-SyncLog ('Sync check failed: {0}' -f $errorMessage)
      }
    }
    Start-Sleep -Seconds 3
  }
} finally {
  if ($ownsMutex) { $mutex.ReleaseMutex() }
  $mutex.Dispose()
}
