@echo off
setlocal
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

if exist "target\release\aurora-browser.exe" (
  start "" "%~dp0target\release\aurora-browser.exe"
  exit /b
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust was not found. See WINDOWS-QUICKSTART.md for the build requirements.
  pause
  exit /b 1
)

cargo run --release --locked --features custom-protocol
if errorlevel 1 (
  echo Aurora could not start. Review the error above and WINDOWS-QUICKSTART.md.
  pause
  exit /b 1
)
