@echo off
chcp 65001 >nul
color 0A
title AgentWhipper - 一键启动

:menu
cls
echo ===================================================
echo             AgentWhipper - AI 智能体鞭策器
echo ===================================================
echo.

call :ensure_built
if %ERRORLEVEL% EQU 0 (
    echo [INFO] 编译完成，准备启动...
) else (
    echo [WARN] 当前未能完成编译，可修复环境后重试，或直接关闭窗口退出。
)
echo.
echo 请选择要执行的操作:
echo 1. 启动监控模式 (whip start codex --mode watch)
echo 2. 查看鞭打统计 (whip stats)
echo 3. 手动抽一鞭子并检测运行中的 Agent (whip whip)
echo 4. 查看所有预设 (whip preset list)
echo 5. 保持窗口开启 (关闭窗口才退出)
echo.

choice /c 12345 /n /m "请输入选项 (1-5): "
if errorlevel 5 goto keep_alive
if errorlevel 4 goto show_presets
if errorlevel 3 goto do_whip
if errorlevel 2 goto show_stats
if errorlevel 1 goto start_watch
goto menu

:start_watch
cls
call :ensure_built
if %ERRORLEVEL% NEQ 0 (
    pause
    goto menu
)
target\release\whip.exe start codex --mode watch
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
echo [INFO] 启动器会持续等待输入。
echo [INFO] 如需退出，请直接关闭当前窗口。
echo.
pause
goto menu

:ensure_built
echo [INFO] 正在同步最新代码并编译 release 版本，请稍候...
cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] 编译失败，请检查 Rust 环境是否正确安装！
    exit /b 1
)

exit /b 0
