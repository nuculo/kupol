//! Security Dropout — Randomized Fault Injection for Plugin Quality Diagnostics
//!
//! Вдохновлён `lib/kan/dropout.ml` из OCaml KAN.
//! В нейросетях Dropout случайно обнуляет нейроны с вероятностью p,
//! чтобы предотвратить переобучение. Мы применяем ту же идею к плагинам безопасности:
//!
//! - С вероятностью `p` случайный плагин «отключается» на один прогон.
//! - Если находки исчезают → плагин реально полезен (Unique Coverage).
//! - Если находки не меняются → плагин дублирует другие (Dead Plugin, можно удалить).
//!
//! Это позволяет автоматически диагностировать качество набора OWASP плагинов.

use rand::Rng;

/// Режим работы Dropout
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropoutMode {
    /// Все плагины активны (Production)
    Disabled,
    /// Случайное отключение с вероятностью p (Diagnostics)
    Enabled { p: f64 },
}

/// Security Dropout Layer
pub struct SecurityDropout {
    pub mode: DropoutMode,
}

impl SecurityDropout {
    /// Создать Dropout слой
    pub fn new(mode: DropoutMode) -> Self {
        Self { mode }
    }

    /// Production mode: все плагины работают
    pub fn production() -> Self {
        Self { mode: DropoutMode::Disabled }
    }

    /// Diagnostics mode: случайное отключение с p=0.3
    pub fn diagnostics() -> Self {
        Self { mode: DropoutMode::Enabled { p: 0.3 } }
    }

    /// Применяет Dropout маску к списку активных плагинов.
    /// Возвращает отфильтрованный список и имена отключённых плагинов.
    pub fn apply_mask<'a>(&self, expert_names: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
        match self.mode {
            DropoutMode::Disabled => {
                // Все проходят — Production режим
                (expert_names.to_vec(), vec![])
            }
            DropoutMode::Enabled { p } => {
                let mut rng = rand::rng();
                let mut active = Vec::new();
                let mut dropped = Vec::new();

                for &name in expert_names {
                    if rng.random::<f64>() < p {
                        // Нейрон «обнулён» — плагин отключён на этот прогон
                        dropped.push(name);
                    } else {
                        // Нейрон жив — плагин работает нормально
                        active.push(name);
                    }
                }

                // Гарантируем, что хотя бы 1 плагин остался активным
                if active.is_empty() && !expert_names.is_empty() {
                    active.push(expert_names[0]);
                    dropped.retain(|&x| x != expert_names[0]);
                }

                (active, dropped)
            }
        }
    }
}
