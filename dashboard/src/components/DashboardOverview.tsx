import { ShieldCheck, Scan, Clock, FileCode, Activity, Loader2 } from 'lucide-react';
import RiskGauge from './RiskGauge';
import type { ScanResult } from '../App';

interface Props {
  scanResult: ScanResult | null;
  scanHistory: ScanResult[];
  onRunScan: (path: string) => void;
  isScanning: boolean;
}

export default function DashboardOverview({ scanResult, scanHistory, onRunScan, isScanning }: Props) {
  return (
    <div className="w-full h-full overflow-auto p-6">
      <div className="max-w-6xl mx-auto space-y-6">

        {/* Hero Section */}
        {!scanResult && (
          <div className="rounded-2xl border border-white/10 bg-gradient-to-br from-[rgba(57,255,20,0.05)] to-[rgba(0,243,255,0.05)] p-12 text-center">
            <div className="w-20 h-20 mx-auto rounded-2xl bg-gradient-to-br from-[#39ff14] to-[#00f3ff] flex items-center justify-center mb-6 shadow-[0_0_40px_rgba(57,255,20,0.4)] animate-glow">
              <ShieldCheck size={40} className="text-black" />
            </div>
            <h2 className="text-3xl font-bold text-white mb-3">
              Duo Architecture Guardian
            </h2>
            <p className="text-gray-400 text-lg mb-8 max-w-lg mx-auto">
              AI-powered security scanning for GitLab Merge Requests.
              Detect vulnerabilities before they reach production.
            </p>
            <button
              onClick={() => onRunScan('src/')}
              disabled={isScanning}
              className="px-8 py-4 rounded-xl text-base font-bold bg-gradient-to-r from-[#39ff14] to-[#00f3ff] text-black hover:opacity-90 transition-opacity disabled:opacity-50 shadow-[0_0_40px_rgba(57,255,20,0.4)]"
            >
              {isScanning ? <><Loader2 size={20} className="animate-spin inline mr-2" />Scanning...</> : '🔍 Run Your First Scan'}
            </button>
          </div>
        )}

        {/* Dashboard with Results */}
        {scanResult && (
          <>
            {/* Top Row: Stats + Gauge */}
            <div className="grid grid-cols-12 gap-6">
              {/* Stat Cards */}
              <div className="col-span-8 grid grid-cols-4 gap-4">
                <StatCard
                  icon={<FileCode size={20} className="text-[#00f3ff]" />}
                  label="Files Scanned"
                  value={scanResult.summary.total_files}
                  color="text-[#00f3ff]"
                />
                <StatCard
                  icon={<Scan size={20} className="text-[#39ff14]" />}
                  label="Total Findings"
                  value={scanResult.summary.total_findings}
                  color="text-[#39ff14]"
                />
                <StatCard
                  icon={<Clock size={20} className="text-purple-400" />}
                  label="Duration"
                  value={`${scanResult.duration_ms}ms`}
                  color="text-purple-400"
                />
                <StatCard
                  icon={<Activity size={20} className="text-orange-400" />}
                  label="Plugins Active"
                  value={8}
                  color="text-orange-400"
                />
              </div>

              {/* Risk Gauge */}
              <div className="col-span-4 rounded-xl border border-white/10 bg-white/[0.02] p-4 flex items-center justify-center">
                <RiskGauge score={scanResult.summary.risk_score} />
              </div>
            </div>

            {/* Severity Breakdown */}
            <div className="rounded-xl border border-white/10 bg-white/[0.02] p-6">
              <h3 className="text-sm font-bold text-gray-400 uppercase tracking-widest mb-4">Severity Breakdown</h3>
              <div className="space-y-3">
                {[
                  { label: 'Critical', count: scanResult.summary.critical, color: 'bg-red-500', total: scanResult.summary.total_findings },
                  { label: 'High', count: scanResult.summary.high, color: 'bg-orange-500', total: scanResult.summary.total_findings },
                  { label: 'Medium', count: scanResult.summary.medium, color: 'bg-yellow-500', total: scanResult.summary.total_findings },
                  { label: 'Low', count: scanResult.summary.low, color: 'bg-green-500', total: scanResult.summary.total_findings },
                  { label: 'Info', count: scanResult.summary.info, color: 'bg-blue-400', total: scanResult.summary.total_findings },
                ].map(item => (
                  <div key={item.label} className="flex items-center gap-4">
                    <span className="w-16 text-xs text-gray-400">{item.label}</span>
                    <div className="flex-1 h-2 rounded-full bg-white/5 overflow-hidden">
                      <div
                        className={`h-full rounded-full ${item.color} transition-all duration-500`}
                        style={{ width: `${item.total > 0 ? (item.count / item.total) * 100 : 0}%` }}
                      />
                    </div>
                    <span className="w-10 text-right text-sm font-mono text-white">{item.count}</span>
                  </div>
                ))}
              </div>
            </div>

            {/* Recent Scans */}
            {scanHistory.length > 1 && (
              <div className="rounded-xl border border-white/10 bg-white/[0.02] p-6">
                <h3 className="text-sm font-bold text-gray-400 uppercase tracking-widest mb-4">Scan History</h3>
                <div className="space-y-2">
                  {scanHistory.slice(0, 5).map((s, i) => (
                    <div key={s.id} className={`flex items-center justify-between px-4 py-3 rounded-lg ${i === 0 ? 'bg-[rgba(57,255,20,0.05)] border border-[rgba(57,255,20,0.15)]' : 'bg-white/[0.02]'}`}>
                      <div className="flex items-center gap-3">
                        <span className="text-xs font-mono text-gray-500">{s.id}</span>
                        <span className="text-sm text-gray-300">{s.target}</span>
                      </div>
                      <div className="flex items-center gap-4 text-xs text-gray-400">
                        <span>{s.summary.total_findings} findings</span>
                        <span className={s.summary.risk_score >= 9 ? 'text-red-400 font-bold' : s.summary.risk_score >= 7 ? 'text-orange-400' : 'text-green-400'}>
                          Risk: {s.summary.risk_score.toFixed(1)}
                        </span>
                        <span>{s.duration_ms}ms</span>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </>
        )}

        {/* Feature Cards */}
        <div className="grid grid-cols-3 gap-4 pb-6">
          {[
            { icon: '🔍', title: '8 Security Plugins', desc: 'SQL Injection, Unsafe Code, Hardcoded Secrets, XSS, Crypto, and more' },
            { icon: '📊', title: 'CVSS Risk Scoring', desc: 'Quantitative risk assessment with blast-radius analysis' },
            { icon: '🏗️', title: '12 Actor System', desc: 'AST Analysis, Drift Detection, Babylonian Scanning, Swarm Agents' },
          ].map(feat => (
            <div key={feat.title} className="rounded-xl border border-white/10 bg-white/[0.02] p-5 hover:bg-white/[0.04] transition-colors">
              <div className="text-2xl mb-3">{feat.icon}</div>
              <h3 className="text-sm font-bold text-white mb-1">{feat.title}</h3>
              <p className="text-xs text-gray-400 leading-relaxed">{feat.desc}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function StatCard({ icon, label, value, color }: { icon: React.ReactNode; label: string; value: string | number; color: string }) {
  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.02] p-4 flex flex-col gap-2 hover:bg-white/[0.04] transition-colors">
      <div className="flex items-center gap-2 text-xs text-gray-400">
        {icon}
        <span>{label}</span>
      </div>
      <div className={`text-2xl font-bold ${color}`}>{value}</div>
    </div>
  );
}
