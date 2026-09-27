use tokio::sync::mpsc;

/// A single MR event (webhook tick)
#[derive(Debug, Clone)]
pub struct MRTick {
    pub mr_id: u64,
    pub author: String,
    pub changed_files: usize,
    pub timestamp_ms: u64,
}

/// Signal emitted by the Consumer after analyzing a window
#[derive(Debug, Clone)]
pub enum AnalyticsSignal {
    Normal { window_size: usize, avg_files: f64 },
    AnomalyDetected { reason: String, author: String, window_size: usize },
}

// =============================================================================
// 1. PRODUCER: Simulates live MR webhook feed (like `start-market-feed`)
// =============================================================================

pub async fn start_mr_feed(ticks: Vec<MRTick>, tick_tx: mpsc::Sender<MRTick>, delay_ms: u64) {
    for tick in ticks {
        tracing::info!("🌊 [Producer] MR-{} from '{}' ({} files)", tick.mr_id, tick.author, tick.changed_files);
        let _ = tick_tx.send(tick).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
    }
    drop(tick_tx); // Close channel → signals end of session
}

// =============================================================================
// 2. SLIDING WINDOW BUFFER (like `sliding-window-buffer`)
// =============================================================================

pub async fn sliding_window_buffer(
    window_size: usize,
    mut tick_rx: mpsc::Receiver<MRTick>,
    window_tx: mpsc::Sender<Vec<MRTick>>,
) {
    let mut buffer: Vec<MRTick> = Vec::new();
    
    while let Some(tick) = tick_rx.recv().await {
        buffer.push(tick);
        
        if buffer.len() < window_size {
            // Burn-in period: accumulating window
            continue;
        }
        
        // Window is full → send downstream, then slide
        let _ = window_tx.send(buffer.clone()).await;
        buffer.remove(0); // Slide: drop oldest
    }
    
    drop(window_tx); // Propagate close
}

// =============================================================================
// 3. CONSUMER: Analytics Worker (like `kan-inference-worker`)
// =============================================================================

pub async fn analytics_consumer(
    mut window_rx: mpsc::Receiver<Vec<MRTick>>,
    signal_tx: mpsc::Sender<AnalyticsSignal>,
) {
    while let Some(window) = window_rx.recv().await {
        let n = window.len();
        let avg_files = window.iter().map(|t| t.changed_files as f64).sum::<f64>() / n as f64;
        
        // Anomaly: same author authored all MRs in the window
        let first_author = &window[0].author;
        let same_author = window.iter().all(|t| t.author == *first_author);
        
        let signal = if same_author && n >= 3 {
            AnalyticsSignal::AnomalyDetected {
                reason: format!("Burst: {} consecutive MRs from single author (avg {:.1} files)", n, avg_files),
                author: first_author.clone(),
                window_size: n,
            }
        } else {
            AnalyticsSignal::Normal { window_size: n, avg_files }
        };
        
        let _ = signal_tx.send(signal).await;
    }
    
    drop(signal_tx);
}

// =============================================================================
// 4. DEMO: Full Pipeline Orchestration
// =============================================================================

pub async fn demo_streaming_pipeline() {
    tracing::info!("🌊 [Streaming] Initializing 3-stage core.async pipeline...");
    
    // Channels (like Clojure `chan`)
    let (tick_tx, tick_rx) = mpsc::channel::<MRTick>(10);
    let (window_tx, window_rx) = mpsc::channel::<Vec<MRTick>>(10);
    let (signal_tx, mut signal_rx) = mpsc::channel::<AnalyticsSignal>(10);
    
    // Simulated live MR stream
    let ticks = vec![
        MRTick { mr_id: 501, author: "alice".into(), changed_files: 3, timestamp_ms: 1000 },
        MRTick { mr_id: 502, author: "alice".into(), changed_files: 7, timestamp_ms: 2000 },
        MRTick { mr_id: 503, author: "alice".into(), changed_files: 2, timestamp_ms: 3000 },
        MRTick { mr_id: 504, author: "bob".into(),   changed_files: 15, timestamp_ms: 4000 },
        MRTick { mr_id: 505, author: "alice".into(), changed_files: 1, timestamp_ms: 5000 },
        MRTick { mr_id: 506, author: "carol".into(), changed_files: 4, timestamp_ms: 6000 },
    ];
    
    let window_size = 3;
    
    // Spawn pipeline stages
    tokio::spawn(start_mr_feed(ticks, tick_tx, 50));
    tokio::spawn(sliding_window_buffer(window_size, tick_rx, window_tx));
    tokio::spawn(analytics_consumer(window_rx, signal_tx));
    
    // Read signals (blocking consumer, like Clojure `<!!`)
    while let Some(signal) = signal_rx.recv().await {
        match &signal {
            AnalyticsSignal::Normal { window_size, avg_files } => {
                tracing::info!("   ✅ [Signal] Normal (window={}, avg_files={:.1})", window_size, avg_files);
            }
            AnalyticsSignal::AnomalyDetected { reason, author, .. } => {
                tracing::warn!("   🚨 [Signal] ANOMALY from '{}': {}", author, reason);
            }
        }
    }
    
    tracing::info!("🌊 [Streaming] Pipeline gracefully shut down.");
}
