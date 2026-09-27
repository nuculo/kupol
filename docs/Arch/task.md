# Задачи: карта фаз движка

> Этот документ содержит полную карту всех 50-ти фаз разработки платформы **Duo Agents**, с точным маппингом файлов, модулей и демо-сценариев. В связи с рефакторингом в Phase 50, весь интеллектуальный код переехал в `duo-kan/src/`.

---

## 🛠️ Базовая Архитектура (Open Shell)

- [x] **Phase 1: CLI + Scan Pipeline** — Запуск и оркетрирование (promptfoo-стиль). `duo-agents/src/cli/`
- [x] **Phase 2: API endpoints (Axum)** — Вебхуки GitLab и WebSocket. `duo-agents/src/server/`
- [x] **Phase 3: Build/Run scripts** — `run.sh`, `build.sh`
- [x] **Phase 4: Dashboard UI (React)** — Фронтенд `/dashboard`
- [x] **Phase 5: GitLab Integration** — `duo-agents/src/fetcher.rs`, `GITLAB_TOKEN`
- [x] **Phase 6: GitLab CI/CD Pipeline Integration** — `duo-agents init (.gitlab-ci.yml)`
- [x] **Phase 7: React Graph Visualization (Blast Radius)** — `GET /api/graph` `duo-kan/src/blast_radius.rs`
- [x] **Phase 8: MCP Server Integration** — Json-RPC stdio мост `duo-agents/src/mcp_server.rs`

---

## 🧠 KAN Intelligence V1-V4 (Closed Core: `duo-kan/src/`)

- [x] **Phase 9: Graph-KAN Trust Decay (G-KAN)** — Использование `bspline.rs` на ребрах графа для деградации доверия.
- [x] **Phase 10: Mixture of Experts (MoE) Router** — `duo-kan/src/scan/moe_router.rs`. Выбор Top-K экспертов-плагинов.
- [x] **Phase 11: Temporal KAN (Boiling Frog 🐸)** — `duo-kan/src/kan/tkan.rs`. Скрытое накопление уязвимостей через `hidden_state`.
- [x] **Phase 12: Security Phi-Protocol (Полиморфные Плагины)** — `duo-kan/src/scan/phi.rs`. RegexPhi, SemanticPhi, StructuralPhi.
- [x] **Phase 13: Sparse Router (Inverted Security Index)** — `duo-kan/src/scan/sparse_router.rs`. Маршрутизация на основе `HashMap<keyword, weights>`.
- [x] **Phase 14: Security Dropout (Dead Plugin Detection)** — `duo-kan/src/scan/dropout.rs`. Случайное отключение плагинов для защиты от переобучения.
- [x] **Phase 15: DPO Loss (Preference-Based Risk Scoring)** — `duo-kan/src/kan/dpo.rs`. Адаптивный скоринг.
- [x] **Phase 16: Multi-Head Security Attention** — `duo-kan/src/kan/attention.rs`. (ControlFlow, DataFlow, Historical).
- [x] **Phase 17: Checkpoint/Resume (Crash Resilience)** — `duo-kan/src/scan/checkpoint.rs`. Сохранение прогресса KAN-графа.
- [x] **Phase 18: Rayon Parallelism (Multi-Core Scan)** — `par_iter` в сканере для использования всех ядер.
- [x] **Phase 19: Hybrid Symbolic Discovery (Auto-Rule Generation)** — `duo-kan/src/kan/symbolic.rs`. (Демо 5)
- [x] **Phase 20: Agent KAN (ε-Greedy Self-Evolving Plugins)** — `duo-kan/src/kan/agent.rs`. Саморазвивающиеся RL-агенты.
- [x] **Phase 21: WAL Scan Audit Trail** — `duo-kan/src/scan/wal.rs`. Логовые-структуры (Write-Ahead-Log).
- [x] **Phase 22: RAG Vulnerability Knowledge Base** — `duo-kan/src/kan/rag.rs` (TF-IDF + cosine search).
- [x] **Phase 23: Gradient Clipping + Early Stopping** — `duo-kan/src/kan/training_utils.rs`.
- [x] **Phase 24: Lazy DAG Pipeline (XLA-style)** — `duo-kan/src/scan/dag.rs`. Вычисление графа отложенно.
- [x] **Phase 25: LoRA Fine-Tuning for Plugin Rules** — `duo-kan/src/kan/lora.rs`. Адаптеры для базовых правил.
- [x] **Phase 26: SSE Streaming Findings** — `duo-kan/src/scan/stream.rs`. Live-поток обнаружений.
- [x] **Phase 27: KAT DSL — Composable Rule Algebra** — `duo-kan/src/scan/dsl.rs`. Алгебра правил И/ИЛИ/XOR.
- [x] **Phase 28: Nucleus Fuzzy Matching** — `duo-kan/src/scan/fuzzy.rs`. Нечеткий поиск символов.
- [x] **Phase 29: Sparse MoE Routing (Inverted Index)** — `duo-kan/src/scan/sparse_router.rs`. (Повтор Фазы 13).
- [x] **Phase 30: Entity CRDT (Collaborative Knowledge Graph)** — `duo-kan/src/scan/crdt.rs`. Разрешение конфликтов при анализе MR.
- [x] **Phase 31: Neural ODE (Continuous-Time Risk Dynamics)** — `duo-kan/src/scan/ode.rs`. Непрерывное нарастание риска по времени коммита.
- [x] **Phase 32: Tensor Embedding (Factorized 3D Profiling)** — `duo-kan/src/scan/tensor_profile.rs`. 3-мерная база профиля.
- [x] **Phase 33: Scalar Quantization (INT8 Compressed Engine)** — `duo-kan/src/scan/quantize.rs`. 4x сжатие весов.
- [x] **Phase 34: Type-Safe Rule Engine (PhantomData / GADT)** — `duo-kan/src/scan/gadt_rule.rs`. Защита от багов на уровне типов Rust.

---

## 🔬 v5: Haskell KAN Architecture Integration (Idea 27 - Idea 31)

- [x] **Phase 35: Graph KAN Routing (PetGraph + KAN on Edges)** — `duo-kan/src/scan/graph_kan.rs`. Идея 27.
- [x] **Phase 36: Forward AD Sensitivity Analysis (Dual Numbers)** — `duo-kan/src/scan/forward_ad.rs`. Идея 28.
- [x] **Phase 37: LoRA Edge Adapters for Anomaly Surges** — `duo-kan/src/scan/lora_edge.rs`. Идея 29.
- [x] **Phase 38: What-If Scenario Modeling (Incremental Cache)** — `duo-kan/src/scan/what_if.rs`. Идея 30.
- [x] **Phase 39: Gravity Attention (Einstein Metric)** — `duo-kan/src/scan/gravity.rs`. Идея 31. **(Демо 44 в `run_demos`)**

---

## 🔭 v6-v7: Haskell KAN Deep Dive (Babylon, Uber, PSI)

> *Полная реализация 8 сложнейших математических и поведенческих концептов (Идеи 32–41):*

- [x] **Phase 40: Acceleration Detector (d²E/dt² Jerk Security)** — `duo-kan/src/scan/accel_detector.rs`. Идея 32 ➡️ **Демо 45**.
- [x] **Phase 41: Babylon Basis Cache (O(log n) Spline Lookup)** — `duo-kan/src/scan/basis_cache.rs`. Идея 33 ➡️ **Демо 46**.
- [x] **Phase 42: Surge Pricing for Scan Queue (Demand-Driven)** — `duo-kan/src/scan/surge_queue.rs`. Идея 34 ➡️ **Демо 47**.
- [x] **Phase 43: Adaptive Grid Extension (Plateau Detect → Refine)** — `duo-kan/src/scan/adaptive_grid.rs`. Идея 35 ➡️ **Демо 48**.
- [x] **Phase 44: Gravitational Redshift (Semantic Frequency Shift)** — `duo-kan/src/scan/redshift.rs`. Идея 36 ➡️ **Демо 49**.
- [x] **Phase 45: KV-Cache для Инкрементального Сканирования** — `duo-kan/src/scan/kv_cache.rs`. Идея 37 ➡️ **Демо 50**.
- [x] **Phase 46: Reverse-Mode AD (Analytical Backprop)** — `duo-kan/src/scan/reverse_ad.rs`. Идея 38 ➡️ **Демо 51**.
- [x] **Phase 47: Differentiable Policy (KAN as RL Agent)** — `duo-kan/src/scan/scan_policy.rs`. Идея 39 ➡️ **Демо 52**.
- [x] **Phase 48: Frozen/Trainable Split (Polymorphic AD)** — `duo-kan/src/scan/frozen_trainable.rs`. Идея 40 ➡️ **Демо 53**.
- [x] **Phase 49: Top-K Security Sampling** — `duo-kan/src/scan/topk_sampler.rs`. Идея 41 ➡️ **Демо 54**.

---

## 📦 Phase 50: Closed Core Refactoring (Workspace Split)
> Последний глобальный рефакторинг для подготовки к Open Source релизу оболочки.

- [x] Создать Cargo Workspace: `[workspace] members = ["duo-kan"]`
- [x] Переместить все модули `kan`, `kan_intelligence`, `scan` (Фазы 9-49), а также `models`, `protocol`, `semantic_engine` в `duo-kan/src`.
- [x] Настроить зависимости в `duo-kan/Cargo.toml`.
- [x] Исправить относительные пути к файлам (`fetcher.rs` -> `../../src/test_api.rs`).
- [x] Применить хитрый паттерн `pub use duo_kan::*;` внутри `duo-agents/src/main.rs`. Это позволяет оболочке обращаться к `crate::models` и `crate::scan` так же, как если бы они находились локально, избегая 2000+ правок по всем файлам проекта.
- [x] Убедиться, что `run_demos()` (Фазы 35-49) корректно выведен из `main.rs` в файл `duo-agents/src/demos/mod.rs`.
- [x] Успешная сборка воркспейса (`cargo check --workspace` -> 0 ошибок).
- [x] Полностью чистая и готовая архитектура **Closed Core (`duo-kan`) / Open Shell (`duo-agents`)**.
