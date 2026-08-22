@echo off
setlocal

if "%~1"=="" (
  echo Token Poker MCP failed: missing server entrypoint. 1>&2
  exit /b 64
)

if exist "%~dp0..\bin\codex-app-server.exe" (
  set "TOKEN_HOLDEM_CODEX_APP_SERVER_PATH=%~dp0..\bin\codex-app-server.exe"
)

if defined CODEX_MCP_NODE_PATH if exist "%CODEX_MCP_NODE_PATH%" (
  "%CODEX_MCP_NODE_PATH%" %*
  exit /b
)
if defined CODEX_BROWSER_USE_NODE_PATH if exist "%CODEX_BROWSER_USE_NODE_PATH%" (
  "%CODEX_BROWSER_USE_NODE_PATH%" %*
  exit /b
)
if defined CODEX_ELECTRON_RESOURCES_PATH if exist "%CODEX_ELECTRON_RESOURCES_PATH%\cua_node\bin\node.exe" (
  "%CODEX_ELECTRON_RESOURCES_PATH%\cua_node\bin\node.exe" %*
  exit /b
)
if defined CODEX_CLI_PATH for %%I in ("%CODEX_CLI_PATH%") do if exist "%%~dpIcua_node\bin\node.exe" (
  "%%~dpIcua_node\bin\node.exe" %*
  exit /b
)
if defined USERPROFILE if exist "%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe" (
  "%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe" %*
  exit /b
)
if defined LOCALAPPDATA for /d %%D in ("%LOCALAPPDATA%\OpenAI\Codex\runtimes\cua_node\*") do if exist "%%~fD\bin\node.exe" (
  "%%~fD\bin\node.exe" %*
  exit /b
)

where node >nul 2>&1
if not errorlevel 1 (
  node %*
  exit /b
)

echo Token Poker MCP failed: no compatible Codex Node runtime was found. 1>&2
exit /b 127
