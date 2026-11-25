@echo off
rem Double-click to fetch the latest Pine code and start the app.
cd /d "%~dp0"
rem When started from the Update button, give Pine a moment to close first.
if "%~1"=="--wait" ping -n 3 127.0.0.1 >nul
echo Getting the latest version...
git pull --ff-only
if errorlevel 1 (
  echo.
  echo Could not update. Check your internet connection, or send this window to Claude.
  pause
  exit /b 1
)
rem Keep everything above this line byte for byte: an older Cortex (or Pine) runs this file while
rem git pull replaces it, and Windows keeps reading from the same position.
echo Building Cortex. The first build takes a few minutes, later ones are quick...
cargo build -p pine
if errorlevel 1 (
  pause
  exit /b 1
)
rem After the first build you can open target\debug\pine.exe (Cortex) directly or pin it.
rem The program keeps the name pine.exe so that older copies can still update themselves.
rem Cortex then updates itself from its Update button, with no console window.
start "" "%~dp0target\debug\pine.exe"
