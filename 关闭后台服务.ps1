$studioRoot = $PSScriptRoot
$backendPath = Join-Path $studioRoot 'desktop\native\itasha-backend.exe'
$studioServers = Get-CimInstance Win32_Process | Where-Object {
    ($_.Name -eq 'itasha-backend.exe' -and $_.ExecutablePath -eq $backendPath) -or
    ($_.Name -eq 'python.exe' -and $_.ExecutablePath -eq (Join-Path $studioRoot 'runtime\python.exe') -and $_.CommandLine -match 'studio_server.py')
}
foreach ($studioServer in $studioServers) {
    Stop-Process -Id $studioServer.ProcessId -ErrorAction SilentlyContinue
}
