import { useState, useMemo } from 'react';
import { Search, Filter, ChevronDown, ChevronUp, AlertTriangle, FileCode, Scan, Loader2 } from 'lucide-react';
import type { ScanResult } from '../App';
import RiskGauge from './RiskGauge';

interface Props {
  scanResult: ScanResult | null;
  onRunScan: (path: string) => void;
  isScanning: boolean;
}

const severityConfig = {
  Critical: { color: 'text-red-400', bg: 'bg-red-500/20', border: 'border-red-500/30', dot: 'bg-red-500' },
  High:     { color: 'text-orange-400', bg: 'bg-orange-500/20', border: 'border-orange-500/30', dot: 'bg-orange-500' },
  Medium:   { color: 'text-yellow-400', bg: 'bg-yellow-500/20', border: 'border-yellow-500/30', dot: 'bg-yellow-500' },
  Low:      { color: 'text-green-400', bg: 'bg-green-500/20', border: 'border-green-500/30', dot: 'bg-green-500' },
  Info:     { color: 'text-blue-400', bg: 'bg-blue-500/20', border: 'border-blue-500/30', dot: 'bg-blue-400' },
};

export default function ScanResults({ scanResult, onRunScan, isScanning }: Props) {
  const [search, setSearch] = useState('');
  const [severityFilter, setSeverityFilter] = useState<string>('all');
  const [pluginFilter, setPluginFilter] = useState<string>('all');
  const [sortField, setSortField] = useState<'severity' | 'file' | 'plugin'>('severity');
  const [sortAsc, setSortAsc] = useState(false);
  const [expandedRow, setExpandedRow] = useState<number | null>(null);

  // No results view
  if (!scanResult) {
    return (
      <div className="w-full h-full flex items-center justify-center">
        <div className="text-center space-y-6">
          <div className="w-20 h-20 mx-auto rounded-2xl bg-gradient-to-br from-[#39ff14]/20 to-[#00f3ff]/20 border border-[rgba(57,255,20,0.2)] flex items-center justify-center">
            <Scan size={40} className="text-[#39ff14] opacity-60" />
          </div>
          <div>
            <h2 className="text-2xl font-bold text-white mb-2">No Scan Results Yet</h2>
            <p className="text-gray-400 text-sm">Run a security scan to see vulnerability findings</p>
          </div>
          <button
            onClick={() => onRunScan('src/')}
            disabled={isScanning}
            className="px-6 py-3 rounded-xl text-sm font-semibold bg-gradient-to-r from-[#39ff14] to-[#00f3ff] text-black hover:opacity-90 transition-opacity disabled:opacity-50 shadow-[0_0_30px_rgba(57,255,20,0.3)]"
          >
            {isScanning ? <><Loader2 size={16} className="animate-spin inline mr-2" />Scanning...</> : '🔍 Scan src/ Directory'}
          </button>
        </div>
      </div>
    );
  }

  const sevOrder: Record<string, number> = { Critical: 5, High: 4, Medium: 3, Low: 2, Info: 1 };

  // Unique plugins
  const plugins = useMemo(() => {
    const set = new Set(scanResult.findings.map(f => f.plugin));
    return Array.from(set).sort();
  }, [scanResult]);

  // Filter + sort
  const filtered = useMemo(() => {
    let items = scanResult.findings;

    if (search) {
      const q = search.toLowerCase();
      items = items.filter(f =>
        f.title.toLowerCase().includes(q) ||
        f.file.toLowerCase().includes(q) ||
        f.plugin.toLowerCase().includes(q) ||
        (f.cwe && f.cwe.toLowerCase().includes(q))
      );
    }
    if (severityFilter !== 'all') {
      items = items.filter(f => f.severity === severityFilter);
    }
    if (pluginFilter !== 'all') {
      items = items.filter(f => f.plugin === pluginFilter);
    }

    items = [...items].sort((a, b) => {
      let cmp = 0;
      if (sortField === 'severity') cmp = (sevOrder[b.severity] || 0) - (sevOrder[a.severity] || 0);
      else if (sortField === 'file') cmp = a.file.localeCompare(b.file);
      else if (sortField === 'plugin') cmp = a.plugin.localeCompare(b.plugin);
      return sortAsc ? -cmp : cmp;
    });
    return items;
  }, [scanResult, search, severityFilter, pluginFilter, sortField, sortAsc]);

  const toggleSort = (field: typeof sortField) => {
    if (sortField === field) setSortAsc(!sortAsc);
    else { setSortField(field); setSortAsc(false); }
  };

  const SortIcon = ({ field }: { field: typeof sortField }) => (
    sortField === field
      ? (sortAsc ? <ChevronUp size={14} /> : <ChevronDown size={14} />)
      : <ChevronDown size={14} className="opacity-30" />
  );

  return (
    <div className="w-full h-full flex flex-col p-6 gap-4 overflow-hidden">

      {/* Top Bar: Stats + Gauge */}
      <div className="flex items-start gap-6 shrink-0">
        {/* Summary Cards */}
        <div className="flex-1 grid grid-cols-5 gap-3">
          {(['Critical', 'High', 'Medium', 'Low', 'Info'] as const).map(sev => {
            const count = scanResult.summary[sev.toLowerCase() as keyof typeof scanResult.summary] as number;
            const cfg = severityConfig[sev];
            return (
              <button
                key={sev}
                onClick={() => setSeverityFilter(severityFilter === sev ? 'all' : sev)}
                className={`rounded-xl p-4 border transition-all duration-200 ${
                  severityFilter === sev
                    ? `${cfg.bg} ${cfg.border} ring-1 ring-${cfg.dot}`
                    : 'bg-white/[0.02] border-white/10 hover:bg-white/[0.05]'
                }`}
              >
                <div className="flex items-center gap-2 mb-1">
                  <div className={`w-2 h-2 rounded-full ${cfg.dot}`} />
                  <span className={`text-xs font-medium ${cfg.color}`}>{sev}</span>
                </div>
                <div className="text-2xl font-bold text-white">{count}</div>
              </button>
            );
          })}
        </div>
        {/* Risk Gauge */}
        <div className="shrink-0">
          <RiskGauge score={scanResult.summary.risk_score} />
        </div>
      </div>

      {/* Scan Meta */}
      <div className="flex items-center justify-between text-xs text-gray-500 shrink-0">
        <div className="flex items-center gap-4">
          <span>📁 {scanResult.summary.total_files} files • {scanResult.summary.total_findings} findings • {scanResult.duration_ms}ms</span>
          <span className="text-gray-600">|</span>
          <span>{scanResult.id}</span>
        </div>
        <div className="flex items-center gap-2">
          <span>{filtered.length} of {scanResult.findings.length} shown</span>
        </div>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-3 shrink-0">
        <div className="relative flex-1 max-w-md">
          <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-500" />
          <input
            type="text"
            value={search}
            onChange={e => setSearch(e.target.value)}
            placeholder="Search findings..."
            className="w-full pl-10 pr-4 py-2 rounded-lg bg-white/[0.04] border border-white/10 text-sm text-white placeholder:text-gray-500 focus:outline-none focus:border-[#39ff14]/40 focus:ring-1 focus:ring-[#39ff14]/20 transition"
          />
        </div>
        <div className="flex items-center gap-2">
          <Filter size={14} className="text-gray-500" />
          <select
            value={pluginFilter}
            onChange={e => setPluginFilter(e.target.value)}
            className="px-3 py-2 rounded-lg bg-white/[0.04] border border-white/10 text-sm text-gray-300 focus:outline-none focus:border-[#39ff14]/40 transition appearance-none cursor-pointer"
          >
            <option value="all">All Plugins</option>
            {plugins.map(p => <option key={p} value={p}>{p}</option>)}
          </select>
        </div>
      </div>

      {/* Table */}
      <div className="flex-1 overflow-auto rounded-xl border border-white/10 bg-white/[0.01]">
        <table className="w-full text-sm">
          <thead className="sticky top-0 bg-[#0d0d14]/95 backdrop-blur border-b border-white/10 z-10">
            <tr className="text-xs text-gray-400 uppercase tracking-wider">
              <th className="px-4 py-3 text-left w-10">#</th>
              <th className="px-4 py-3 text-left cursor-pointer select-none hover:text-white" onClick={() => toggleSort('severity')}>
                <span className="flex items-center gap-1">Severity <SortIcon field="severity" /></span>
              </th>
              <th className="px-4 py-3 text-left cursor-pointer select-none hover:text-white" onClick={() => toggleSort('plugin')}>
                <span className="flex items-center gap-1">Plugin <SortIcon field="plugin" /></span>
              </th>
              <th className="px-4 py-3 text-left">Title</th>
              <th className="px-4 py-3 text-left cursor-pointer select-none hover:text-white" onClick={() => toggleSort('file')}>
                <span className="flex items-center gap-1">File <SortIcon field="file" /></span>
              </th>
              <th className="px-4 py-3 text-left w-16">Line</th>
              <th className="px-4 py-3 text-left w-24">CWE</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((f, i) => {
              const cfg = severityConfig[f.severity];
              const isExpanded = expandedRow === i;
              return (
                <>
                  <tr
                    key={i}
                    onClick={() => setExpandedRow(isExpanded ? null : i)}
                    className={`border-b border-white/5 cursor-pointer transition-colors duration-100 ${
                      isExpanded ? 'bg-white/[0.05]' : 'hover:bg-white/[0.03]'
                    }`}
                  >
                    <td className="px-4 py-3 text-gray-500 font-mono text-xs">{i + 1}</td>
                    <td className="px-4 py-3">
                      <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-xs font-semibold ${cfg.bg} ${cfg.color} border ${cfg.border}`}>
                        <span className={`w-1.5 h-1.5 rounded-full ${cfg.dot}`} />
                        {f.severity}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-gray-300 font-mono text-xs">{f.plugin}</td>
                    <td className="px-4 py-3 text-white">{f.title}</td>
                    <td className="px-4 py-3">
                      <span className="flex items-center gap-1.5 text-gray-400">
                        <FileCode size={13} className="text-gray-500 shrink-0" />
                        <span className="font-mono text-xs truncate max-w-[200px]">{f.file}</span>
                      </span>
                    </td>
                    <td className="px-4 py-3 text-gray-400 font-mono text-xs">{f.line ?? ''}</td>
                    <td className="px-4 py-3">
                      {f.cwe && <span className="text-xs font-mono text-blue-300/60">{f.cwe}</span>}
                    </td>
                  </tr>
                  {isExpanded && (
                    <tr key={`${i}-detail`} className="bg-white/[0.03]">
                      <td colSpan={7} className="px-8 py-4">
                        <div className="space-y-3 max-w-3xl">
                          <p className="text-sm text-gray-300">{f.description}</p>
                          {f.code_snippet && (
                            <pre className="bg-black/60 border border-white/10 rounded-lg p-3 text-xs font-mono text-gray-300 overflow-x-auto">
                              {f.code_snippet}
                            </pre>
                          )}
                          {f.suggestion && (
                            <div className="flex items-start gap-2 text-xs text-yellow-300/80 bg-yellow-500/10 border border-yellow-500/20 rounded-lg p-3">
                              <AlertTriangle size={14} className="shrink-0 mt-0.5" />
                              <span>{f.suggestion}</span>
                            </div>
                          )}
                        </div>
                      </td>
                    </tr>
                  )}
                </>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
