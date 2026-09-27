#!/usr/bin/env bash
# ============================================================================
# 🚀 run.sh — Запуск Duo Architecture Guardian
# ============================================================================
# Использование:
#   ./run.sh              # Запуск Web UI + API (http://localhost:3000)
#   ./run.sh scan         # Сканирование текущего проекта
#   ./run.sh scan <path>  # Сканирование указанного пути
#   ./run.sh demo         # Запуск демо-сценариев (20 demos)
#   ./run.sh info         # Информация о системе
#   ./run.sh report       # Экспорт отчёта
#   ./run.sh help         # Справка
# ============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Detect binary
if [ -f "target/release/duo-agents" ]; then
    BIN="target/release/duo-agents"
elif [ -f "target/debug/duo-agents" ]; then
    BIN="target/debug/duo-agents"
else
    echo -e "${RED}❌ Бинарник не найден. Запустите сначала: ./build.sh${NC}"
    exit 1
fi

CMD="${1:-serve}"
shift 2>/dev/null || true

case "$CMD" in
    serve|ui|web|start)
        echo -e "${CYAN}"
        echo "  ╔══════════════════════════════════════════════════════╗"
        echo "  ║  🛡️  Duo Architecture Guardian — Web Server          ║"
        echo "  ╚══════════════════════════════════════════════════════╝"
        echo -e "${NC}"
        $BIN serve --port "${PORT:-3000}" "$@"
        ;;

    scan)
        TARGET="${1:-src/}"
        shift 2>/dev/null || true
        $BIN scan --path "$TARGET" "$@"
        ;;

    scan-json)
        TARGET="${1:-src/}"
        shift 2>/dev/null || true
        $BIN scan --path "$TARGET" --format json "$@"
        ;;

    scan-md)
        TARGET="${1:-src/}"
        shift 2>/dev/null || true
        $BIN scan --path "$TARGET" --format markdown "$@"
        ;;

    demo)
        $BIN demo "$@"
        ;;

    info)
        $BIN info
        ;;

    report)
        $BIN report "$@"
        ;;

    help|-h|--help)
        echo -e "${CYAN}"
        echo "  ╔══════════════════════════════════════════════════════╗"
        echo "  ║  🛡️  Duo Architecture Guardian — Help                ║"
        echo "  ╚══════════════════════════════════════════════════════╝"
        echo -e "${NC}"
        echo -e "  ${GREEN}serve${NC}  (default)    Web UI + API (http://localhost:3000)"
        echo -e "  ${GREEN}scan${NC}   [path]       Сканирование кода на уязвимости"
        echo -e "  ${GREEN}scan-json${NC} [path]    Скан с JSON-выводом"
        echo -e "  ${GREEN}scan-md${NC}  [path]     Скан с Markdown-выводом"
        echo -e "  ${GREEN}demo${NC}               Запуск 20 демо-сценариев"
        echo -e "  ${GREEN}info${NC}               Информация о системе"
        echo -e "  ${GREEN}report${NC}             Экспорт последнего отчёта"
        echo -e "  ${GREEN}help${NC}               Эта справка"
        echo ""
        echo -e "  ${CYAN}Примеры:${NC}"
        echo -e "    ./run.sh                     ${YELLOW}# Web UI + API${NC}"
        echo -e "    ./run.sh scan                ${YELLOW}# Скан текущего проекта${NC}"
        echo -e "    ./run.sh scan /path/to/code  ${YELLOW}# Скан указанного пути${NC}"
        echo -e "    ./run.sh scan-json src/ > report.json"
        echo -e "    PORT=8080 ./run.sh           ${YELLOW}# Кастомный порт${NC}"
        echo ""
        ;;

    *)
        echo -e "${RED}❌ Неизвестная команда: $CMD${NC}"
        echo "Используйте: ./run.sh help"
        exit 1
        ;;
esac
