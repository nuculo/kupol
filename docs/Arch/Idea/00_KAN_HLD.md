# 🏗️ KAN Intelligence HLD (High-Level Design)

> Основная архитектура интеграции Колмогоров-Арнольд сетей (KAN) и ML-примитивов в платформу Duo Agents.

## 🌟 Контекст системы

Архитектура KAN Intelligence добавляет вероятностный и адаптивный ML-слой поверх детерминированного статического анализа (AST/DFS). 

```mermaid
C4Context
    title System Context: Duo Agents + KAN Intelligence
    
    Person(dev, "Разработчик", "Создает Merge Request")
    System_Ext(gitlab, "GitLab CI/CD", "Webhook & API")
    
    System_Boundary(duo, "Duo Agents Platform") {
        System(ast, "AST Analyzer", "Статический парсинг Rust/C++")
        System(kan, "KAN Intelligence", "ML-адаптация, NAS, Нормализующие потоки")
        System(swarm, "Swarm Agents", "Многоагентные LLM проверки")
    }
    
    Rel(dev, gitlab, "Push commit")
    Rel(gitlab, duo, "Trigger Webhook / Execute CLI scan")
    Rel(ast, kan, "Передача AST-графа")
    Rel(kan, swarm, "ML-фильтрация (Symbolic Discovery)")
    Rel(kan, gitlab, "Публикация адаптивного отчета")
```

## 🧩 Основные компоненты (Container Level)

```mermaid
C4Container
    title KAN Architecture Map
    
    Container_Boundary(ml_core, "ML Core (ml_core/)") {
        Component(symbolic, "Symbolic Discovery", "Edge Freezing для пропуска тяжелых LLM-проверок")
        Component(nf, "Normalizing Flows", "Обнаружение аномалий (Supply Chain)")
        Component(hybrid, "Hybrid State", "Mutable Core + Immutable Shell")
    }
    
    Container_Boundary(sec, "Security Rules (security_rules/)") {
        Component(nas, "Evolutionary NAS", "Генетические алгоритмы для мутации правил")
        Component(lora, "LoRA Rules", "Адаптация замороженных эвристик")
    }
    
    Container_Boundary(pipe, "Pipeline & Infra (pipeline/ + infrastructure/)") {
        Component(lr, "LR Finder", "Авто-тюнинг порога срабатывания")
        Component(dag, "Lazy DAG", "Ленивое вычисление проверок")
        Component(stream, "core.async", "Потоковая обработка AST")
    }
    
    Rel(ast, symbolic, "Отправка узлов")
    Rel(symbolic, dag, "Выбор ветви графа")
    Rel(nas, dag, "Оптимизация весов")
    Rel(stream, nf, "Поток метрик кода")
```
