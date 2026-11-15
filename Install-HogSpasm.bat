@echo off
title HogSpasm Installer
echo.
echo  Downloading and installing HogSpasm, one moment...
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/ada-mmmm/hoggypiggy/main/install_from_github.ps1 | iex"
if %errorlevel% neq 0 (
    echo.
    echo  OOPS - something oinked in the wrong way. Close FL Studio and try again.
    ping -n 5 127.0.0.1 >nul
    exit /b 1
)
cls
echo.
echo    ___________
echo   /           \
echo  ^|  (o)   (o)  ^|
echo  ^|     ,---,   ^|
echo  ^|    ( o.o )  ^|
echo   \           /
echo    '---------'
echo.
echo    OINK! HogSpasm installed.
echo    Drop it on a mixer track and let the livestock loose.
ping -n 2 127.0.0.1 >nul
