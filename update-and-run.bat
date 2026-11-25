@echo off
rem Double-click to fetch the latest Pine code, build it and start it.
rem After the first build you can also open target\debug\pine.exe directly (or pin it);
rem Pine updates itself from its Update button, with no console window.
cd /d "%~dp0"
echo Getting the latest version...
git pull --ff-only
if errorlevel 1 (
  echo.
  echo Could not update. Check your internet connection, or send this window to Claude.
  pause
  exit /b 1
)
echo Building Pine. The first build takes a few minutes, later ones are quick...
cargo build -p pine
if errorlevel 1 (
  pause
  exit /b 1
)
start "" "%~dp0target\debug\pine.exe"
