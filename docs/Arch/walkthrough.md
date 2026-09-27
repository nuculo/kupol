# KUPOL — Walkthrough

## Что было создано

Security scanner for a local repo — CLI, REST API, React dashboard, and Rust actors.

Архитектура вдохновлена подходом **promptfoo**: Плагин → Оценщик (Grader) → Пайплайн Отчета.

## Скриншоты Дашборда

````carousel
![Главная страница — Hero-секция с кнопкой "Запустить Сканирование" и карточками фич](/home/timur/.gemini/antigravity/brain/ee1226dd-8757-4e8f-a078-d583fd6ba2e6/landing_page_1774165762259.png)
<!-- slide -->
![Обзорный Дашборд — 59 файлов, 124 находки, 174мс, Шкала Риска 10.0 CRITICAL, графики серьезности](/home/timur/.gemini/antigravity/brain/ee1226dd-8757-4e8f-a078-d583fd6ba2e6/dashboard_overview_1774165814103.png)
<!-- slide -->
![Таблица Результатов Сканирования — сортируемые/фильтруемые уязвимости с бейджами серьезности, плагинами и CWE ссылками](/home/timur/.gemini/antigravity/brain/ee1226dd-8757-4e8f-a078-d583fd6ba2e6/scan_results_table_1774165821238.png)
<!-- slide -->
![Граф Архитектуры — визуализация Радиуса Поражения (Угасания Доверия) с автоматической раскладкой Dagre](/home/timur/.gemini/antigravity/brain/ee1226dd-8757-4e8f-a078-d583fd6ba2e6/architecture_graph_v1_1774179822217.png)
````

![Полная запись сессии](/home/timur/.gemini/antigravity/brain/ee1226dd-8757-4e8f-a078-d583fd6ba2e6/graph_visualization_fixed_final_1774179798992.webp)

## Визуализация Графа в React (Этап 7)

Наш Дашборд теперь включает выделенную **Вкладку Архитектуры**, полностью интегрированную с Rust бекендом (`/api/graph`).
Она использует `@xyflow/react` и графовую библиотеку `dagre` для умного позиционирования узлов:
- **Автоматическая Раскладка**: Отображение структуры `EntityGraph` (Классы, Модули, Эндпоинты, SQL запросы) в реальном времени.
- **Индикатор Радиуса Поражения**: Использует BFS алгоритм "Trust Decay" (Угасание Доверия) для точного выявления уязвимостей. Когда компонент (например, `unsafe_raw_query()`) вызывает критическую уязвимость, UI точно подсвечивает, какие API эндпоинты и сервисы функционально затронуты этим — узлы получают неоново-красные анимированные рамки (свойство `isTainted`).

## Созданные Файлы

| Файл | Строк (LOC) | Описание |
|------|-----|-------------|
| **Бекенд** | | |
| `src/cli.rs` | 95 | CLI интерфейс (clap): scan, serve, report, demo, info |
| `src/scan/mod.rs` | 180 | Пайплайн сканирования: Severity, Finding, ScanResult, run_scan() |
| `src/scan/plugins.rs` | 400 | 8 Плагинов Безопасности (SecurityPlugins) |
| `src/scan/graders.rs` | 32 | Система CVSS скоринга рисков + фильтрация по серьезности |
| `src/scan/report.rs` | 140 | Таблица в терминале, экспорт в JSON, Markdown |
| **Дашборд** | | |
| `dashboard/src/App.tsx` | 145 | Навигация по 3 вкладкам + интеграция API |
| `dashboard/src/components/ScanResults.tsx` | 210 | Таблица с фильтрацией и сортировкой находок |
| `dashboard/src/components/RiskGauge.tsx` | 85 | SVG-спидометр со стрелкой для отображения общего риска |
| `dashboard/src/components/DashboardOverview.tsx` | 160 | Статистика + графики серьезности + история |
| **Скрипты** | | |
| `build.sh` | 95 | Сборка Rust бекенда + Дашборда |
| `run.sh` | 115 | Запуск: serve, scan, demo, info |

## 8 Плагинов Безопасности

| Плагин | Серьезность | CWE | Обнаруживает |
|--------|----------|-----|---------|
| `sql-injection` | CRITICAL | CWE-89 | SQL через format!/f-string |
| `hardcoded-secrets` | CRITICAL | CWE-798 | API ключи, токены, пароли |
| `unsafe-code` | HIGH | CWE-676 | unsafe блоки, сырые указатели (raw pointers) |
| `deprecated-api` | HIGH | CWE-477 | eval, exec, innerHTML, transmute |
| `input-validation` | HIGH | CWE-20 | from_raw_parts, проверки длин |
| `crypto-weakness` | HIGH | CWE-327 | MD5, SHA1, DES, RC4 |
| `todo-fixme` | MEDIUM | — | TODO, FIXME, HACK, XXX |
| `unwrap-panic` | LOW | CWE-391 | Вызовы .unwrap() без обработки ошибок |

## Интеграция с GitLab (Этапы 5 и 6)

KUPOL встраивается в пайплайны GitLab CI/CD (опциональный git-хост, как GitHub) и пишет отчёт по изменениям.

**Шаг 1: Генерация Пайплайна**
В корне вашего репозитория запустите команду `init` для автоматической генерации преднастроенного файла `.gitlab-ci.yml`:
```bash
./run.sh init
```

**Шаг 2: Настройка Авторизации**
Убедитесь, что вы задали переменную `GITLAB_TOKEN` в настройках CI/CD Variables вашего репозитория. Пайплайн автоматически подхватит системные переменные среды `$CI_MERGE_REQUEST_IID` и `$CI_PROJECT_ID`.

При каждом пуше кода сканер выполнит:
```bash
duo-agents scan src/ --mr $CI_MERGE_REQUEST_IID --project $CI_PROJECT_ID
```
Это обеспечивает мгновенное "Red Team" ревью кода без трения, прямо в стандартном рабочем процессе разработчика, публикуя Markdown-отчет о рисках текстом прямо в комментариях к MR.

## Интеграция MCP Сервера (Model Context Protocol) (Этап 8)

KUPOL может опционально запускаться как **MCP Сервер** (через stdio JSON-RPC мост), что позволяет любой MCP-совместимой среде (Cursor, Claude Desktop, AI-агенты) нативно использовать его возможности сканирования локально.

**Запуск сервера:**
```json
{
  "mcpServers": {
    "duo-agents": {
      "command": "/path/to/duo-agents",
      "args": ["mcp"]
    }
  }
}
```

Текущие экспортируемые инструменты (Tools):
- **`scan_codebase(path)`**: Запускает архитектурный сканер уязвимостей на Rust по переданному локальному пути к проекту, и возвращает полный профиль рисков вместе с найденными уязвимостями.

## Результаты Тестирования

- ✅ `cargo build` — 1.3с, 0 ошибок
- ✅ `npm run build` — 229мс, 0 TS ошибок
- ✅ CLI `scan src/` — 124 находки в 59 файлах (174мс) — 28 Critical, 32 High, 25 Medium, 32 Low, 7 Info
- ✅ CLI `info` — отображает все 8 плагинов, 12 акторов
- ✅ API `/api/health`, `/api/scan`, `/api/scans`, `/api/plugins`
- ✅ Дашборд — все 3 вкладки функционируют с живым API

## Быстрый Старт

```bash
cd duo-agents
./build.sh              # Сборка проекта (Rust + Дашборд)
./run.sh                # Запуск Web UI на localhost:3000
./run.sh scan           # Запуск сканирования в CLI терминале
./run.sh scan-json src/ # JSON экспорт логов
./run.sh demo           # Запуск 20-ти демонстрационных сценариев акторов
```
