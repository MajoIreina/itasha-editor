@echo off
setlocal
cd /d "%~dp0next"
if not exist "target\release\itasha-desktop.exe" (
  echo NEXT executable not found. Build it in the next directory:
  echo   pnpm install --frozen-lockfile
  echo   pnpm desktop:build
  pause
  exit /b 1
)
start "" /d "%~dp0next" "%~dp0next\target\release\itasha-desktop.exe"
