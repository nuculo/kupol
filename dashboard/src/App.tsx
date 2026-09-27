import { useState, useCallback } from 'react';
import { ShieldCheck, Activity, Scan, LayoutGrid, GitGraph, Loader2 } from 'lucide-react';
import GraphViewer from './GraphViewer';
import ScanResults from './components/ScanResults';
import DashboardOverview from './components/DashboardOverview';
import './index.css';

type Tab = 'dashboard' | 'scan' | 'graph';

// Types matching Rust backend
export interface Finding {
  plugin: string;
  severity: 'Critical' | 'High' | 'Medium' | 'Low' | 'Info';
  title: string;
  description: string;
  file: string;
  line: number | null;
  code_snippet: string | null;
  suggestion: string | null;
  cwe: string | null;
}

export interface ScanSummary {
  total_files: number;
  total_findings: number;
  critical: number;
  high: number;
  medium: number;
  low: number;
  info: number;
  risk_score: number;
}

export interface ScanResult {
  id: string;
  timestamp: string;
  target: string;
  duration_ms: number;
  findings: Finding[];
  summary: ScanSummary;
}

function App() {
  const [activeTab, setActiveTab] = useState<Tab>('dashboard');
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [isScanning, setIsScanning] = useState(false);
  const [scanHistory, setScanHistory] = useState<ScanResult[]>([]);

  const runScan = useCallback(async (path: string = 'src/') => {
    setIsScanning(true);
    try {
      const res = await fetch('/api/scan', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path }),
      });
      const data: ScanResult = await res.json();
      setScanResult(data);
      setScanHistory(prev => [data, ...prev]);
      setActiveTab('scan');
    } catch (err) {
      console.error('Scan failed:', err);
    } finally {
      setIsScanning(false);
    }
  }, []);

  const tabs: { id: Tab; label: string; icon: typeof LayoutGrid }[] = [
    { id: 'dashboard', label: 'Dashboard', icon: LayoutGrid },
    { id: 'scan', label: 'Scan Results', icon: Scan },
    { id: 'graph', label: 'Architecture', icon: GitGraph },
  ];

  return (
    <div className="w-screen h-screen bg-[#0a0a0f] text-gray-200 overflow-hidden flex flex-col font-sans">
      {/* Header */}
      <header className="h-16 border-b border-[rgba(255,255,255,0.1)] flex items-center justify-between px-6 bg-gradient-to-r from-black/80 to-[#0a0a0f]/80 backdrop-blur-md z-50 shrink-0">
        <div className="flex items-center space-x-3">
          <div className="w-9 h-9 rounded-lg bg-gradient-to-br from-[#39ff14] to-[#00f3ff] flex items-center justify-center animate-glow shadow-[0_0_15px_rgba(57,255,20,0.5)]">
            <ShieldCheck size={20} className="text-black" />
          </div>
          <h1 className="text-xl font-bold tracking-tight bg-clip-text text-transparent bg-gradient-to-r from-white to-gray-400">
            KUPOL
          </h1>
          <span className="text-xs text-gray-500 ml-2 hidden sm:inline">v0.1.0</span>
        </div>

        {/* Tabs */}
        <nav className="flex items-center space-x-1">
          {tabs.map(tab => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center space-x-2 px-4 py-2 rounded-lg text-sm font-medium transition-all duration-200 ${
                activeTab === tab.id
                  ? 'bg-[rgba(57,255,20,0.15)] text-[#39ff14] border border-[rgba(57,255,20,0.3)]'
                  : 'text-gray-400 hover:text-white hover:bg-white/5'
              }`}
            >
              <tab.icon size={16} />
              <span>{tab.label}</span>
            </button>
          ))}
        </nav>

        {/* Actions */}
        <div className="flex items-center space-x-3">
          <button
            onClick={() => runScan('src/')}
            disabled={isScanning}
            className="flex items-center space-x-2 px-4 py-2 rounded-lg text-sm font-semibold bg-gradient-to-r from-[#39ff14] to-[#00f3ff] text-black hover:opacity-90 transition-opacity disabled:opacity-50 shadow-[0_0_20px_rgba(57,255,20,0.3)]"
          >
            {isScanning ? <Loader2 size={16} className="animate-spin" /> : <Scan size={16} />}
            <span>{isScanning ? 'Scanning...' : 'Run Scan'}</span>
          </button>
          {scanResult && (
            <div className="flex items-center space-x-2 text-xs font-medium text-[#39ff14] bg-[rgba(57,255,20,0.1)] px-3 py-1.5 rounded-full border border-[rgba(57,255,20,0.2)]">
              <Activity size={14} className="animate-pulse" />
              <span>{scanResult.summary.total_findings} findings</span>
            </div>
          )}
        </div>
      </header>

      {/* Content */}
      <main className="flex-1 overflow-hidden">
        {activeTab === 'dashboard' && (
          <DashboardOverview
            scanResult={scanResult}
            scanHistory={scanHistory}
            onRunScan={runScan}
            isScanning={isScanning}
          />
        )}
        {activeTab === 'scan' && (
          <ScanResults scanResult={scanResult} onRunScan={runScan} isScanning={isScanning} />
        )}
        {activeTab === 'graph' && (
          <section className="w-full h-full relative">
            <GraphViewer />
          </section>
        )}
      </main>
    </div>
  );
}

export default App;
