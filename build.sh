#!/usr/bin/env bash
# ============================================================================
# 🔧 build.sh — Сборка Duo Architecture Guardian
# ============================================================================
# Использование:
#   ./build.sh              # Полная сборка (Rust + Dashboard)
#   ./build.sh --rust       # Только Rust backend
#   ./build.sh --dashboard  # Только React dashboard
#   ./build.sh --release    # Release-оптимизация
#   ./build.sh --clean      # Чистая сборка
# ============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RUST_ONLY=false
DASH_ONLY=false
RELEASE=false
CLEAN=false

for arg in "$@"; do
    case $arg in
        --rust) RUST_ONLY=true ;;
        --dashboard|--ui) DASH_ONLY=true ;;
        --release) RELEASE=true ;;
        --clean) CLEAN=true ;;
        --help|-h)
            echo -e "${CYAN}🔧 Duo Architecture Guardian — Сборка${NC}"
            echo ""
            echo "  --rust         Только Rust backend"
            echo "  --dashboard    Только React dashboard"
            echo "  --release      Release-оптимизация"
            echo "  --clean        Чистая сборка"
            exit 0 ;;
        *) echo -e "${RED}❌ Неизвестный аргумент: $arg${NC}"; exit 1 ;;
    esac
done

echo -e "${CYAN}"
echo "  ╔══════════════════════════════════════════════════════╗"
echo "  ║     🛡️  Duo Architecture Guardian — Сборка          ║"
echo "  ╚══════════════════════════════════════════════════════╝"
echo -e "${NC}"

START_TIME=$(date +%s)

# Clean
if [ "$CLEAN" = true ]; then
    echo -e "${YELLOW}🧹 Чистка...${NC}"
    cargo clean 2>/dev/null || true
    rm -rf dashboard/dist dashboard/node_modules 2>/dev/null || true
    echo -e "${GREEN}✅ Чистка завершена${NC}"
fi

# Rust Backend
if [ "$DASH_ONLY" = false ]; then
    echo -e "${CYAN}▶ Сборка Rust backend...${NC}"
    if [ "$RELEASE" = true ]; then
        cargo build --release 2>&1 | tail -5
        BIN="target/release/duo-agents"
    else
        cargo build 2>&1 | tail -5
        BIN="target/debug/duo-agents"
    fi
    echo -e "${GREEN}✅ Rust backend собран: $BIN${NC}"

    # Verify
    VERSION=$(./$BIN info 2>/dev/null | grep "Guardian" | head -1 || echo "v0.1.0")
    echo -e "${GREEN}   $VERSION${NC}"
fi

# React Dashboard
if [ "$RUST_ONLY" = false ] && [ -d "dashboard" ]; then
    echo ""
    echo -e "${CYAN}▶ Сборка React Dashboard...${NC}"
    cd dashboard
    if [ ! -d "node_modules" ]; then
        npm install --prefer-offline 2>&1 | tail -3
    fi
    npm run build 2>&1 | tail -5
    cd ..
    echo -e "${GREEN}✅ Dashboard собран: dashboard/dist/${NC}"
fi

# Summary
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✅ Сборка завершена за ${ELAPSED}с${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo -e "  ${YELLOW}Запуск:${NC}  ./run.sh"
echo -e "  ${YELLOW}Скан:${NC}    ./run.sh scan"
echo -e "  ${YELLOW}Демо:${NC}    ./run.sh demo"
echo ""
