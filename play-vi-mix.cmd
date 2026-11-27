@echo off
rem vi-mix launcher: double-click, or pass -NativeOutput / -Warm / -Stock / -GameDir "<folder>".
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0play-vi-mix.ps1" %*
pause
