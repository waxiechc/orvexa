@echo off
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\start-p2p.ps1" -OpenBrowser
if errorlevel 1 pause
