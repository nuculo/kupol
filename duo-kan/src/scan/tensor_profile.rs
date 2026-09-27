//! Tensor Embedding — Factorized 3D Profiling
//!
//! Вместо того чтобы представлять уязвимость (finding) как простую строку (SQL Injection),
//! мы описываем её как точку в 3D-тензоре профилирования.
//! 
//! Оси тензора (4x4x4 = 64 размерности):
//! - Layer (UI, API, Backend, DB)
//! - Vector (DataFlow, ControlFlow, Crypto, Auth)
//! - Severity (Low, Medium, High, Critical)
//!
//! Агрегируя все находки проекта в один `ProjectTensor`,
//! мы можем делать Tensor Contraction (свёртку / маргинализацию осей),
//! чтобы находить глубокие структурные инсайты. Например: "У вас глобальные
//! проблемы с Crypto на слое DB".

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer { UI = 0, API = 1, Backend = 2, DB = 3 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vector { DataFlow = 0, ControlFlow = 1, Crypto = 2, Auth = 3 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Low = 0, Medium = 1, High = 2, Critical = 3 }

/// Факторизованный профиль отдельной находки
#[derive(Debug, Clone)]
pub struct VulnProfile {
    pub name: String,
    pub layer: Layer,
    pub vector: Vector,
    pub severity: Severity,
}

/// Агрегированный 3D-тензор проекта.
/// Хранит частоту (счётчик) уязвимостей в каждой координате [Layer][Vector][Severity]
pub struct ProjectTensor {
    // 4x4x4 tensor
    data: [[[u32; 4]; 4]; 4],
}

impl ProjectTensor {
    pub fn new() -> Self {
        Self {
            data: [[[0; 4]; 4]; 4],
        }
    }

    /// Добавить находку в тензор
    pub fn add_vuln(&mut self, vuln: &VulnProfile) {
        let l = vuln.layer as usize;
        let v = vuln.vector as usize;
        let s = vuln.severity as usize;
        self.data[l][v][s] += 1;
    }

    /// Свёртка тензора: Маргинализация оси Severity.
    /// Получаем 2D Матрицу: Layer × Vector
    pub fn contract_severity(&self) -> [[u32; 4]; 4] {
        let mut matrix = [[0; 4]; 4];
        for l in 0..4 {
            for v in 0..4 {
                let mut sum = 0;
                for s in 0..4 {
                    sum += self.data[l][v][s];
                }
                matrix[l][v] = sum;
            }
        }
        matrix
    }

    /// Свёртка тензора: Маргинализация осей Layer и Severity.
    /// Получаем 1D Вектор: агрегированную сумму по Vector (какой вектор атак самый популярный в проекте)
    pub fn contract_layer_and_severity(&self) -> [u32; 4] {
        let mut vec = [0; 4];
        for v in 0..4 {
            let mut sum = 0;
            for l in 0..4 {
                for s in 0..4 {
                    sum += self.data[l][v][s];
                }
            }
            vec[v] = sum;
        }
        vec
    }

    /// Вспомогательная функция вывода 2D Матрицы
    pub fn print_layer_vector_heatmap(&self) {
        let matrix = self.contract_severity();
        let layers = ["UI     ", "API    ", "Backend", "DB     "];
        let vectors = ["DataFlow", "CtrlFlow", "Crypto  ", "Auth    "];
        
        println!("     ║ {} │ {} │ {} │ {}", vectors[0], vectors[1], vectors[2], vectors[3]);
        println!("═════╬══════════╤══════════╤══════════╤══════════");
        
        for l in 0..4 {
            print!("{}║", layers[l]);
            for v in 0..4 {
                let val = matrix[l][v];
                let cell = if val == 0 { "    .   ".to_string() } else { format!("  {:>3}   ", val) };
                print!("{}│", cell);
            }
            println!("");
        }
    }

    pub fn print_vector_distribution(&self) {
        let vec = self.contract_layer_and_severity();
        let vectors = ["DataFlow", "ControlFlow", "Cryptography", "Authentication"];
        
        println!("Агрегированный вектор архитектурных слабостей:");
        for v in 0..4 {
            let bars = "█".repeat(vec[v] as usize);
            println!("{:<15}: {:>3} {}", vectors[v], vec[v], bars);
        }
    }
}
