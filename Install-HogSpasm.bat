@echo off
title HogSpasm Installer
setlocal
set "ZIP=%TEMP%\HogSpasm.vst3.zip"
set "SRC=%TEMP%\HogSpasm-extract"
set "SYS=C:\Program Files\Common Files\VST3\HogSpasm.vst3"
set "USR=%LOCALAPPDATA%\Programs\Common\VST3\HogSpasm.vst3"

echo.
echo  Downloading the hog from GitHub...
curl.exe -L -f -# -o "%ZIP%" https://github.com/ada-mmmm/hoggypiggy/releases/latest/download/HogSpasm.vst3.zip
if errorlevel 1 (
    echo  curl is missing or failed - trying the fallback download...
    powershell -NoProfile -Command "Invoke-WebRequest -Uri 'https://github.com/ada-mmmm/hoggypiggy/releases/latest/download/HogSpasm.vst3.zip' -OutFile '%ZIP%' -UseBasicParsing"
)
if not exist "%ZIP%" goto fail

if exist "%SRC%" rmdir /s /q "%SRC%" >nul 2>&1
mkdir "%SRC%" >nul 2>&1
echo  Unpacking...
tar.exe -xf "%ZIP%" -C "%SRC%" >nul 2>&1
if not exist "%SRC%\HogSpasm.vst3\Contents\x86_64-win\HogSpasm.vst3" goto fail

echo  Installing...
if exist "%SYS%" rmdir /s /q "%SYS%" >nul 2>&1
robocopy "%SRC%\HogSpasm.vst3" "%SYS%" /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP >nul
if errorlevel 8 goto peruser
set "WHERE=%SYS%"
goto verify

:peruser
echo  No admin rights for the system folder - installing for this user instead...
if exist "%USR%" rmdir /s /q "%USR%" >nul 2>&1
robocopy "%SRC%\HogSpasm.vst3" "%USR%" /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP >nul
if errorlevel 8 goto fail
set "WHERE=%USR%"

:verify
if not exist "%WHERE%\Contents\x86_64-win\HogSpasm.vst3" goto fail
if exist "%SRC%" rmdir /s /q "%SRC%" >nul 2>&1
if exist "%ZIP%" del "%ZIP%" >nul 2>&1

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
exit /b 0

:fail
echo.
echo  OOPS - something oinked in the wrong way. Close FL Studio and try again.
ping -n 5 127.0.0.1 >nul
exit /b 1
