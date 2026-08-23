@echo off
setlocal EnableExtensions DisableDelayedExpansion
title Token Poker Installer

pushd "%~dp0" >nul 2>&1
if errorlevel 1 (
    echo Token Poker could not open the extracted package directory.
    set "TOKEN_POKER_EXIT_CODE=2"
    goto :finish
)

set "TOKEN_POKER_INSTALLER=%~dp0install-token-poker.ps1"
call :require_file "manifest.json"
if errorlevel 1 goto :incomplete_package
call :require_file "install-token-poker.ps1"
if errorlevel 1 goto :incomplete_package
call :require_file ".agents\plugins\marketplace.json"
if errorlevel 1 goto :incomplete_package
call :require_file "plugins\token-holdem\.mcp.json"
if errorlevel 1 goto :incomplete_package
call :require_file "plugins\token-holdem\.codex-plugin\plugin.json"
if errorlevel 1 goto :incomplete_package
call :require_file "plugins\token-holdem\release-files.json"
if errorlevel 1 goto :incomplete_package

set "TOKEN_POKER_POWERSHELL=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%TOKEN_POKER_POWERSHELL%" (
    where pwsh.exe >nul 2>&1
    if errorlevel 1 (
        echo Token Poker requires Windows PowerShell or PowerShell 7 to install.
        set "TOKEN_POKER_EXIT_CODE=3"
        goto :finish
    )
    set "TOKEN_POKER_POWERSHELL=pwsh.exe"
)

if defined LOCALAPPDATA goto :log_in_local_app_data
if defined TEMP goto :log_in_temporary_directory
set "TOKEN_POKER_LOG_FILE=%~dp0installer.log"
goto :log_path_ready

:log_in_local_app_data
set "TOKEN_POKER_LOG_FILE=%LOCALAPPDATA%\TokenPoker\logs\installer.log"
goto :log_path_ready

:log_in_temporary_directory
set "TOKEN_POKER_LOG_FILE=%TEMP%\TokenPoker\logs\installer.log"

:log_path_ready

echo Installing or repairing Token Poker...
echo This may take a few minutes while Codex runtime files are prepared.
echo.

"%TOKEN_POKER_POWERSHELL%" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%TOKEN_POKER_INSTALLER%" -LogPath "%TOKEN_POKER_LOG_FILE%"
set "TOKEN_POKER_EXIT_CODE=%ERRORLEVEL%"

echo.
if "%TOKEN_POKER_EXIT_CODE%"=="0" (
    echo Token Poker was installed successfully.
    echo Fully restart Codex, create a new task, and ask: Open Token Poker.
) else (
    echo Token Poker installation failed with exit code %TOKEN_POKER_EXIT_CODE%.
)
echo Installer log: "%TOKEN_POKER_LOG_FILE%"

goto :finish

:incomplete_package
echo Token Poker cannot be installed because the package is incomplete.
echo Extract the entire ZIP into a normal folder, then run this file again.
echo Missing: %TOKEN_POKER_MISSING_FILE%
set "TOKEN_POKER_EXIT_CODE=2"

:finish
if not defined TOKEN_POKER_EXIT_CODE set "TOKEN_POKER_EXIT_CODE=1"
if not defined TOKEN_POKER_INSTALLER_NO_PAUSE (
    echo.
    pause
)
popd >nul 2>&1
endlocal & exit /b %TOKEN_POKER_EXIT_CODE%

:require_file
if exist "%~dp0%~1" exit /b 0
set "TOKEN_POKER_MISSING_FILE=%~1"
exit /b 1
