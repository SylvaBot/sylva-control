@echo off
rem Double-click to fetch the latest Cuepoint code and start the app.
cd /d "%~dp0"
echo Getting the latest version...
git pull --ff-only
if errorlevel 1 (
  echo.
  echo Could not update. Check your internet connection, or send this window to Claude.
  pause
  exit /b 1
)
echo Building and starting Cuepoint...
cargo run -p cuepoint
if errorlevel 1 pause
