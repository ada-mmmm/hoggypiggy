@echo off
title HogSpasm Installer
echo Downloading and installing HogSpasm...
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/ada-mmmm/hoggypiggy/main/install_from_github.ps1 | iex"
echo.
pause
