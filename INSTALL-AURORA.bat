@echo off
setlocal
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

if exist "target\release\bundle\nsis\Aurora_1.7.0_x64-setup.exe" (
  start "" "%~dp0target\release\bundle\nsis\Aurora_1.7.0_x64-setup.exe"
  exit /b
)

where cargo >nul 2>nul
if errorlevel 1 goto :requirements

cargo tauri --version >nul 2>nul
if errorlevel 1 (
  echo Installing the Tauri CLI...
  cargo install tauri-cli --version 2.12.6 --locked
  if errorlevel 1 goto :build_error
)

cargo tauri build --ci --no-sign -- --locked
if errorlevel 1 goto :build_error

set "AURORA_SETUP="
for /f "delims=" %%F in ('dir /b /a-d /o-d "target\release\bundle\nsis\Aurora_*-setup.exe" 2^>nul') do if not defined AURORA_SETUP set "AURORA_SETUP=%%F"
if not defined AURORA_SETUP goto :build_error
start "" "%~dp0target\release\bundle\nsis\%AURORA_SETUP%"
exit /b

:requirements
echo Rust was not found. See WINDOWS-QUICKSTART.md for the build requirements.
pause
exit /b 1

:build_error
echo Aurora installer could not be built. Review the error above.
pause
exit /b 1
