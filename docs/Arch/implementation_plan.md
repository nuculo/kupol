# План: AI Security Agent

## Цель

Собрать сканер безопасности вокруг Rust-backend `duo-agents` (имя crate) с CLI и визуализацией результатов.

**Ключевая идея:** сканер в CI рядом с review изменений.

## Что уже есть (`duo-agents`)

| Компонент | Статус | Строк кода (LOC) |
|-----------|--------|-----|
| 12 Rust-акторов (AST, Security, Drift, Swarm×6...) | ✅ Готов | 8.5K |
| Axum server (:3000) + WebSocket | ✅ Готов | — |
| Dashboard (React + Tailwind + Graph) | ⚠️ Статический | 500 |
| GitLab API клиент | ⚠️ Заглушка | — |
| CLI | ❌ Нет | — |
| Eval Pipeline (promptfoo-стиль) | ❌ Нет | — |
| UI-таблица результатов | ❌ Нет | — |

## Что заимствуем из promptfoo

| Паттерн | Из promptfoo | В наш проект |
|---------|-------------|--------------|
| Plugin → Strategy → Grader | 54 плагина | SecurityPlugin trait с OWASP проверками |
| UI-таблица результатов | React + MUI table | Адаптируем для отображения находок в MR |
| CVSS Risk Scoring | riskScoring.ts | Rust-реализация с учетом blast-radius |
| CLI с subcommands | Commander.js | clap (Rust) |
| Eval Run → JSON → View | eval pipeline | scan → JSON → dashboard |

---

## Предложенные изменения

### 1. Слой CLI (clap)

#### [Новый] `src/cli.rs`
- Командный процессор: `scan`, `serve`, `report`, `demo`
- `scan --mr <id> --project <id>` — сканирование Merge Request через GitLab API
- `serve --port 3000` — запуск Web UI + API сервера
- `report --format json|html|table` — экспорт результатов (CI/CD report)
- `demo` — запуск демо-сценариев (существующие нагрузочные тесты из main.rs)

#### [Изменить] `src/main.rs`
- Вынести демо-сценарии в отдельный модуль `src/demos.rs`
- Использовать main() как CLI роутер через фреймворк clap

#### [Изменить] `Cargo.toml`
- Зависимости: `clap = { version = "4", features = ["derive"] }`
- Зависимости: `chrono`, `tabled` (для форматирования красивых таблиц в терминале)

---

### 2. Пайплайн Сканирования (В стиле Promptfoo)

#### [Новый] `src/scan/mod.rs`
- `ScanConfig` — конфигурация пайплайна безопасности (из YAML)
- `ScanResult` — обертка для результатов (уязвимости, скоринг, метаданные)
- `run_scan(config) -> ScanResult`

#### [Новый] `src/scan/plugins.rs`
- Trait `SecurityPlugin` с обязательными методами: `name()`, `scan()`, `severity()`
- Имплементация плагинов: `SqlInjectionPlugin`, `UnsafeCodePlugin`, `HardcodedSecretsPlugin`, `DependencyPlugin`, `ComplexityPlugin`

#### [Новый] `src/scan/graders.rs`
- Метод `grade_finding(finding) -> GradedFinding` с уровнями серьезности (Critical/High/Medium/Low/Info)
- Система скоринга рисков на основе Blast-Radius (архитектурного поражения) (`blast_radius.rs`)

#### [Новый] `src/scan/report.rs`
- Модуль генераторов JSON, HTML, Markdown экспорта
- Таблица в терминале через `tabled`

---

### 3. Dashboard UI (React + стиль promptfoo)

#### [Изменить] `dashboard/src/App.tsx`
- Инкапсулировать навигацию: **Dashboard** | **Scan Results** | **MR History** | **Settings**
- Установить живое подключение через WebSocket к бекенду Axum

#### [Новый] `dashboard/src/pages/ScanResults.tsx`
- Таблица детализированных находок: Файл | Уязвимость | Серьезность | Score | Разрешение
- Фильтры: по серьезности (severity), по плагину, по имени файла
- Цветовая кодировка: Critical=красный, High=оранжевый, Medium=жёлтый, Low=зелёный

#### [Новый] `dashboard/src/pages/MRHistory.tsx`
- Список просканированных MR с графом трендов (trend chart)

#### [Новый] `dashboard/src/components/RiskGauge.tsx`
- Полукруглая диаграмма для отображения общего риска всего проекта (CVSS-style 0.0 - 10.0)

---

### 4. Интеграция с GitLab API

#### [Изменить] `src/actors/gitlab.rs`
- Обучить GitLab-клиента работе через официальный API, авторизация по `GITLAB_TOKEN`
- Эндпоинт: `GET /api/v4/projects/:id/merge_requests/:mr_iid/changes`
- Постить автоматизированный markdown-комментарий к MR с результатами сканирования и советами

---

### 5. API Endpoints (Axum)

#### [Изменить] `src/main.rs` → внедрение роутера (router)
- `GET /api/scans` — список сканирований
- `GET /api/scans/:id` — детали конкретного запуска
- `POST /api/scan` — запуск нового сканирования в реальном времени
- `GET /api/health` — healthcheck узла
- `GET /ws` — WebSocket (вещание телеметрии)
- Хостинг статичных файлов (Static frontend deployment) из `dashboard/dist/`

---

### 6. Скрипты развертывания

#### [Новый] `build.sh`
- `cargo build --release` + `cd dashboard && npm install && npm run build` (Сборка обоих частей стэка)

#### [Новый] `run.sh`
- `./run.sh` — запуск сервера (dashboard + API) по умолчанию
- `./run.sh scan --mr 300` — сканирование отдельного MR
- `./run.sh demo` — запуск демо-нагрузок
- `./run.sh help` — справка

---

### 7. Этап 6: Интеграция с GitLab CI/CD Pipeline

#### [Изменить] `src/cli.rs`
- Добавить команду `Init` к Subcommand: эквивалент вызова `duo-agents init`
- Аргументы: опционально `--output` или `--force` для перезаписи.

#### [Изменить] `src/main.rs`
- Добавить обработчик для команды `Init`.
- При вызове генерируется файл `.gitlab-ci.yml` в корне проекта (или по указанному пути).

#### [Новый] автогенератор конфигурации `.gitlab-ci.yml`
- Создать встроенную строку (template) в `main.rs`, содержащую:
  - Шаблонный Docker Image (например, `rust:latest`).
  - Stage `security-review`.
  - Правила запуска: `only: - merge_requests`.
  - Установку `duo-agents` (например, `cargo install --path .` или скачивание бинарника с артефактов).
  - Строку запуска: `duo-agents scan . --mr $CI_MERGE_REQUEST_IID --project $CI_PROJECT_ID`.
  - Экспорт переменной `GITLAB_TOKEN` (берется из CI/CD среды).

---

### 8. Этап 7: Визуализация Графа (Blast Radius)

**Цель:** Оживить вкладку "Architecture Graph" на дашборде. Визуализировать граф архитектурных связей (EntityGraph) с подсветкой зараженных узлов (Trust Decay / Blast Radius).

#### [Изменить] `src/main.rs` (Или отдельный API модуль)
- Создать API endpoint `GET /api/graph`.
- Внутри API:
  - Возвращает результаты последнего сканирования и кэшированный `EntityGraph`.
  - Преобразовать наш `EntityGraph` (nodes & edges) в формат JSON, понятный библиотеке `@xyflow/react` (`{ nodes: [{id, type, position, data}], edges: [{id, source, target}] }`).
  - Вычислить Blast Radius для каждого найденного уязвимого звена. Пометить подверженные риску соседние узлы через поле `data.isTainted`.

#### [Изменить] `dashboard/src/components/GraphViewer.tsx`
- Интегрировать библиотеку рендера графов `@xyflow/react`.
- Считывать `/api/graph` (JSON массив узлов и граней).
- Создать специализированные узлы (custom node types): `FunctionNode`, `StructNode`, `EndpointNode`.
- Использовать библиотеку `dagre.js` для авто-расположения узлов на канвасе (layouting), чтобы избежать ручных X/Y координат.
- Подкрашивать узлы красным (неоновым свечением) если `isTainted === true`.
- Подсвечивать ребра (edges) анимированными частицами, чтобы наглядно показывать трассировку данных (Calls/Uses/DependsOn).

---

### 9. Этап 8: Интеграция MCP Сервера

**Цель:** Сделать `duo-agents` доступным в качестве MCP сервера (Model Context Protocol), чтобы среды вроде Cursor, Claude Desktop и другие AI агенты могли вызывать наш анализатор уязвимостей напрямую как свой собственный инструмент (Tool).

#### [Изменить] `src/cli.rs`
- Разработать команду `Mcp` для Subcommand: вызов через `duo-agents mcp`

#### [Новый] `src/mcp_server.rs`
- Реализовать классический `JSON-RPC 2.0` сервер поверх стандартных потоков `stdio` (`stdin().lines()`).
- Реализовать парсер методов (Method Handler):
  - `initialize` → Отправляет серверу `capabilities: { tools: {} }` и идентификатор сервера `duo-agents`.
  - `tools/list` → Сообщает о списке доступных инструментов, в частности: `scan_codebase(path: string)`.
  - `tools/call` → При вызове `scan_codebase`, активирует ядро движка `scan::run_scan(path)` и возвращает результаты в виде JSON-ответной строки.

#### [Изменить] `src/main.rs`
- Интегрировать запуск сервера при передаче `Commands::Mcp` (блокирующий цикл `mcp_server::run_stdio_server().await`).

---

## План верификации (Verification Plan)

### Автоматизированные тесты (Automated Tests)
1. `cargo build` — гарантия компиляции Rust-движка (0 Warnings, 0 Errors).
2. `cargo test` — прогон юнит-тестов (верификация scan пайплайнов и парсеров семантики).
3. `cd dashboard && npm run build` — Сборка React UI через Vite (Проверка статических типов Typescript).

### Ручная верификация (Manual Verification)
1. Выполнить `cargo run -- mcp` в терминале.
2. Подсунуть на вход `stdin` валидный JSON-RPC `initialize` пакет. Проверить, что MCP отвечает ожидаемым профилем `capabilities`.
3. Запустить `tools/list` запрос и эмулировать `tools/call`.

> [!IMPORTANT]
> Точка входа для MCP: `duo-agents mcp`. Этот процесс не должен писать в `stdout` ничего, кроме валидных JSON-RPC пакетов! Любое другое логгирование (предупреждения, инфо логгера) строго перенаправляется в `stderr`, иначе парсеры IDE Cursor и Claude завершат работу с ошибками синтаксиса (SyntaxError / Parsing fails).
