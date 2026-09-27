//! Type-Safe Rule Engine (PhantomData / GADT Simulation)
//!
//! Инспирировано OCaml GADTs (Generalized Algebraic Data Types).
//! В динамических или слабо-типизированных плагинах часто возникает ошибка:
//! Плагин пытается применить "Regex правило строк" к "Структурному узлу AST".
//! Это вызывает panic! или Exception в Runtime.
//!
//! В Rust мы используем Marker Types + PhantomData + Traits,
//! чтобы на уровне компиляции запретить передачу AST-правил в построчный сканер,
//! и наоборот. Неверная композиция просто не скомпилируется.

use std::marker::PhantomData;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Marker Types (Контексты выполнения)
// Мы используем пустые структуры для создания уникальных "Типов Состояния".
// ═══════════════════════════════════════════════════════════════════════════

/// Маркер: Правило предназначено для проверки исходного кода построчно (Строки)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLineCtx;

/// Маркер: Правило предназначено для структурного AST (Синтаксические Деревья)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AstCtx;

/// Маркер: Правило предназначено для HTTP трафика (Заголовки / Тело)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpCtx;

pub trait EvaluableContext {}
impl EvaluableContext for SourceLineCtx {}
impl EvaluableContext for AstCtx {}
impl EvaluableContext for HttpCtx {}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Строго типизированное правило (Алгебраический DSL c PhantomData)
// Тип `<C>` "привязывает" правило к конкретному контексту намертво.
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub enum Rule<C: EvaluableContext> {
    // Базовые узлы (Generic для всех)
    And(Box<Rule<C>>, Box<Rule<C>>),
    Or(Box<Rule<C>>, Box<Rule<C>>),
    Not(Box<Rule<C>>),

    // Примитивы: PhantomData не занимает места в памяти, но "тащит" тип C
    Contains(String, PhantomData<C>),
    RegexMatch(String, PhantomData<C>),

    // AST Специфичные: Вызов этих конструкторов возвращает Rule<AstCtx>
    HasFunctionArgument(String, PhantomData<C>),
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Строго типизированные конструкторы (Builders)
// Они автоматически выводят (Infer) правильный PhantomData.
// ═══════════════════════════════════════════════════════════════════════════

pub fn and<C: EvaluableContext>(left: Rule<C>, right: Rule<C>) -> Rule<C> {
    Rule::And(Box::new(left), Box::new(right))
}

pub fn contains_line(pat: &str) -> Rule<SourceLineCtx> {
    Rule::Contains(pat.to_string(), PhantomData)
}

pub fn contains_ast(pat: &str) -> Rule<AstCtx> {
    Rule::Contains(pat.to_string(), PhantomData)
}

pub fn has_function_argument(arg: &str) -> Rule<AstCtx> {
    Rule::HasFunctionArgument(arg.to_string(), PhantomData)
}

pub fn header_match(pat: &str) -> Rule<HttpCtx> {
    Rule::Contains(pat.to_string(), PhantomData)
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Оценщики (Evaluators) 
// Принимают только правила своего типа.
// ═══════════════════════════════════════════════════════════════════════════

pub struct SourceLineEvaluator;
impl SourceLineEvaluator {
    /// Если сюда передать `Rule<AstCtx>`, компилятор выдаст ошибку `mismatched types`!
    pub fn evaluate(rule: &Rule<SourceLineCtx>, line: &str) -> bool {
        match rule {
            Rule::Contains(pat, _) => line.contains(pat),
            Rule::And(left, right) => Self::evaluate(left, line) && Self::evaluate(right, line),
            Rule::Or(left, right) => Self::evaluate(left, line) || Self::evaluate(right, line),
            Rule::Not(inner) => !Self::evaluate(inner, line),
            Rule::RegexMatch(pat, _) => line.contains(pat), // Mock
            Rule::HasFunctionArgument(_, _) => unreachable!("Ast Rule in SourceLine Evaluator!"), // Если не дай бог
        }
    }
}

pub struct AstEvaluator;
impl AstEvaluator {
    /// Оценивает только правила `Rule<AstCtx>`.
    pub fn evaluate(rule: &Rule<AstCtx>, ast_node: &str) -> bool {
        match rule {
            Rule::Contains(pat, _) => ast_node.contains(pat),
            Rule::HasFunctionArgument(pat, _) => {
                // Mock AST traversal
                ast_node.contains(&format!("arg: {}", pat))
            },
            Rule::And(left, right) => Self::evaluate(left, ast_node) && Self::evaluate(right, ast_node),
            Rule::Or(left, right) => Self::evaluate(left, ast_node) || Self::evaluate(right, ast_node),
            Rule::Not(inner) => !Self::evaluate(inner, ast_node),
            _ => false,
        }
    }
}
