@echo off
chcp 65001 >nul
color 0A
title AgentWhipper - Launcher

:menu
cls
echo ===================================================
echo             AgentWhipper - AI Agent Whipper
echo ===================================================
echo.

call :ensure_built
if %ERRORLEVEL% EQU 0 (
    echo [INFO] Build completed. Ready to start.
) else (
    echo [WARN] Build failed. Fix the environment and retry, or close this window to exit.
)
echo.
echo Choose an action:
echo 1. Watch all detectable agent runtimes (does not require Codex)
echo 2. Show whip stats (whip stats)
echo 3. Manual whip detected running agents (whip whip)
echo 4. List presets (whip preset list)
echo 5. Keep window open (close this window to exit)
echo.

choice /c 12345 /n /m "Enter option (1-5): "
if errorlevel 5 goto keep_alive
if errorlevel 4 goto show_presets
if errorlevel 3 goto do_whip
if errorlevel 2 goto show_stats
if errorlevel 1 goto watch_all
goto menu

:watch_all
cls
call :ensure_built
if %ERRORLEVEL% NEQ 0 (
    pause
    goto menu
)
target\release\whip.exe watch --all
pause
goto menu

:show_stats
cls
call :ensure_built
if %ERRORLEVEL% NEQ 0 (
    pause
    goto menu
)
target\release\whip.exe stats
pause
goto menu

:do_whip
cls
call :ensure_built
if %ERRORLEVEL% NEQ 0 (
    pause
    goto menu
)
target\release\whip.exe whip --preset speedup
pause
goto menu

:show_presets
cls
call :ensure_built
if %ERRORLEVEL% NEQ 0 (
    pause
    goto menu
)
target\release\whip.exe preset list
pause
goto menu

:keep_alive
cls
echo [INFO] Launcher is waiting.
echo [INFO] Close this window to exit.
echo.
pause
goto menu

:ensure_built
echo [INFO] Building latest release. Please wait...
cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Build failed. Please check that Rust is installed correctly.
    exit /b 1
)

exit /b 0
