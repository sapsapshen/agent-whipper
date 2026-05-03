#!/bin/bash

# AgentWhipper 一键启动脚本 (Linux/macOS)

# 设置颜色
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}===================================================${NC}"
echo -e "${CYAN}            AgentWhipper - AI 智能体鞭策器         ${NC}"
echo -e "${CYAN}===================================================${NC}"
echo ""

echo -e "${GREEN}[INFO] 正在同步最新代码并编译 release 版本，请稍候...${NC}"
cargo build --release
if [ $? -ne 0 ]; then
    echo "编译失败，请检查 Rust 环境是否正确安装！"
    exit 1
fi

echo -e "${GREEN}[INFO] 准备就绪！${NC}"
echo ""
echo "请选择要执行的操作:"
echo "1) 启动监控模式 (whip start codex --mode watch)"
echo "2) 查看鞭打统计 (whip stats)"
echo "3) 手动抽一鞭子并检测运行中的 Agent (whip whip)"
echo "4) 查看所有预设 (whip preset list)"
echo "5) 退出"
echo ""

IFS= read -r -n 1 -p "请输入选项 (1-5): " choice
echo ""

case $choice in
    1)
        clear
        ./target/release/whip start codex --mode watch
        ;;
    2)
        clear
        ./target/release/whip stats
        ;;
    3)
        clear
        ./target/release/whip whip --preset speedup
        ;;
    4)
        clear
        ./target/release/whip preset list
        ;;
    5)
        exit 0
        ;;
    *)
        echo "无效选项"
        ;;
esac
