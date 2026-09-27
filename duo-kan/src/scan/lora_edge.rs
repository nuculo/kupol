//! LoRA Edge Adapters for Anomaly Surges
//!
//! Инспирировано транспортной логистикой из Haskell KAN (`lora_adapter.rs`).
//! Если в конкретном модуле (узле графа) вдруг происходит аномальный всплеск уязвимостей
//! (шок коммитов, XSS в React UI компоненте), сканер "навешивает" на этот узел
//! временный легковесный LoRA-адаптер (параметр `boost`).
//!
//! Этот адаптер гиперактивно умножает приоритет сканирования для всех связанных узлов.
//! Когда уязвимости исправлены (или со временем), адаптер плавно затухает (decay).
//! "Самообучающийся рефлекс сканера".

use std::collections::HashMap;

/// Легковесный LoRA-адаптер, навешиваемый на проблемные узлы.
/// В реальной нейросети это матричное умножение W = W_base + A*B.
/// В нашем прототипе это симуляция скалярного "бустера", который самообучается и затухает.
#[derive(Debug, Clone)]
pub struct EdgeAdapter {
    pub active_boost: f64, // Текущий множитель
    pub target_boost: f64, // К какой цели мы сейчас сходимся (обучаемся)
}

impl EdgeAdapter {
    pub fn new() -> Self {
        Self {
            active_boost: 1.0, // Нет аномалий = множитель 1.0
            target_boost: 1.0,
        }
    }

    /// Активирует LoRA из-за аномального количества уязвимостей
    pub fn trigger_anomaly(&mut self, severity: f64) {
        // Устанавливаем новую цель буста (от 1.5 до 3.0 в зависимости от серьезности)
        self.target_boost = 1.0 + severity * 2.0;
    }

    /// Один шаг обучения/эволюции во времени
    pub fn decay_step(&mut self) {
        if self.target_boost > 1.0 {
            // Если аномалия прошла, цель плавно возвращается к 1.0
            self.target_boost = 1.0 + (self.target_boost - 1.0) * 0.5; // Экспоненциальное затухание
        }

        // Адаптер (активный вес) плавно сходится к цели (Эффект Momentum / Learning Rate)
        self.active_boost += (self.target_boost - self.active_boost) * 0.3;
        
        // Ограничитель снизу
        if self.active_boost < 1.0 {
            self.active_boost = 1.0;
        }
    }
}

/// Граф сканирования, поддерживающий временные LoRA-адаптеры
pub struct AnomalyLoraGraph {
    nodes: Vec<String>,
    adapters: HashMap<String, EdgeAdapter>,
    edges: Vec<(String, String)>, // (Source, Target)
}

impl AnomalyLoraGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            adapters: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, name: &str) {
        self.nodes.push(name.to_string());
        self.adapters.insert(name.to_string(), EdgeAdapter::new());
    }

    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.edges.push((from.to_string(), to.to_string()));
    }

    /// Регистрация "Шока" (всплеска уязвимостей в конкретном узле)
    pub fn report_surge(&mut self, node: &str, severity: f64) {
        if let Some(adapter) = self.adapters.get_mut(node) {
            adapter.trigger_anomaly(severity);
        }
    }

    /// Шаг времени (Тик пайплайна)
    pub fn tick(&mut self) {
        for adapter in self.adapters.values_mut() {
            adapter.decay_step();
        }
    }

    /// Вычисляет приоритет сканирования рёбер с учётом базовых весов и LoRA
    pub fn evaluate_edges(&self) -> Vec<(String, String, f64)> {
        let mut results = Vec::new();

        for (from, to) in &self.edges {
            let base_priority = 10.0; // Базовый приоритет всех переходов

            // Активный вес LoRA на узле-источнике (from)
            let lora = self.adapters.get(from).unwrap();
            
            // Итоговый приоритет умножается на адаптер! (Механика LoRA)
            let final_priority = base_priority * lora.active_boost;
            
            results.push((from.clone(), to.clone(), final_priority));
        }

        // Сортируем по убыванию приоритета (самые "горячие" рёбра первыми)
        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        results
    }
    
    pub fn get_adapter_boost(&self, node: &str) -> f64 {
        self.adapters.get(node).map(|a| a.active_boost).unwrap_or(1.0)
    }
}
