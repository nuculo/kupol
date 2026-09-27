//! CLI модуль — clap-based интерфейс командной строки
//!
//! Команды:
//! - `scan`   — запуск сканирования кода
//! - `serve`  — запуск Web UI + API сервера
//! - `report` — экспорт результатов
//! - `demo`   — демо-сценарии

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "duo-agent",
    version = env!("CARGO_PKG_VERSION"),
    about = "🛡️  Duo Architecture Guardian — AI Security Agent for GitLab MR",
    long_about = "AI-powered security scanning platform for GitLab Merge Requests.\nInspired by promptfoo red-team architecture."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Включить подробный вывод
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 🔍 Запуск сканирования кода на уязвимости
    Scan {
        /// Путь к директории или файлу для сканирования
        #[arg(short, long, default_value = ".")]
        path: String,

        /// ID Merge Request в GitLab
        #[arg(long)]
        mr: Option<u64>,

        /// ID проекта в GitLab
        #[arg(long)]
        project: Option<String>,

        /// Формат вывода: table, json, markdown
        #[arg(short, long, default_value = "table")]
        format: String,

        /// Файл для сохранения отчёта
        #[arg(short, long)]
        output: Option<String>,

        /// Минимальный уровень severity для отображения
        #[arg(long, default_value = "low")]
        min_severity: String,
    },

    /// 🌐 Запуск Web UI + API сервера
    Serve {
        /// Порт для сервера
        #[arg(short, long, default_value = "3000")]
        port: u16,

        /// Отключить автооткрытие браузера
        #[arg(long)]
        no_open: bool,
    },

    /// 📊 Экспорт результатов последнего сканирования
    Report {
        /// Формат: json, html, markdown, csv
        #[arg(short, long, default_value = "json")]
        format: String,

        /// Файл для вывода
        #[arg(short, long)]
        output: Option<String>,
    },

    /// 🎭 Запуск демо-сценариев
    Demo {
        /// Номер демо-сценария (0-20, пусто = все)
        #[arg(short, long)]
        number: Option<u32>,
    },

    /// 🛠️  Генерация настроек для GitLab CI/CD (.gitlab-ci.yml)
    Init {
        /// Директория, в которой будет сгенерирован файл (по умолчанию: текущая)
        #[arg(short, long, default_value = ".")]
        dir: String,

        /// Перезаписать существующий файл
        #[arg(short, long)]
        force: bool,
    },

    /// ℹ️  Показать информацию о системе
    Info,

    /// 🤖 Запуск сервера Model Context Protocol (MCP) через stdio
    Mcp,
}
