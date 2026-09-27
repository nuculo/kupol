interface Props {
  score: number; // 0–10
}

export default function RiskGauge({ score }: Props) {
  const clampedScore = Math.min(10, Math.max(0, score));
  const angle = (clampedScore / 10) * 180 - 90; // -90 to 90

  const getColor = () => {
    if (clampedScore >= 9) return { main: '#ef4444', glow: 'rgba(239,68,68,0.5)', label: 'CRITICAL' };
    if (clampedScore >= 7) return { main: '#f97316', glow: 'rgba(249,115,22,0.5)', label: 'HIGH' };
    if (clampedScore >= 4) return { main: '#eab308', glow: 'rgba(234,179,8,0.5)', label: 'MEDIUM' };
    if (clampedScore >= 1) return { main: '#22c55e', glow: 'rgba(34,197,94,0.5)', label: 'LOW' };
    return { main: '#60a5fa', glow: 'rgba(96,165,250,0.5)', label: 'NONE' };
  };

  const { main, glow, label } = getColor();

  // SVG arc gauge
  const radius = 52;
  const cx = 64;
  const cy = 64;

  return (
    <div className="flex flex-col items-center">
      <div className="relative w-[140px] h-[80px] overflow-hidden">
        <svg viewBox="0 0 128 80" className="w-full h-full">
          {/* Background arc */}
          <path
            d={describeArc(cx, cy, radius, -180, 0)}
            fill="none"
            stroke="rgba(255,255,255,0.08)"
            strokeWidth="8"
            strokeLinecap="round"
          />
          {/* Filled arc */}
          <path
            d={describeArc(cx, cy, radius, -180, -180 + (clampedScore / 10) * 180)}
            fill="none"
            stroke={main}
            strokeWidth="8"
            strokeLinecap="round"
            style={{ filter: `drop-shadow(0 0 6px ${glow})`, transition: 'all 0.5s ease' }}
          />
          {/* Needle */}
          <line
            x1={cx}
            y1={cy}
            x2={cx + 38 * Math.cos((angle * Math.PI) / 180)}
            y2={cy + 38 * Math.sin((angle * Math.PI) / 180)}
            stroke="white"
            strokeWidth="2"
            strokeLinecap="round"
            style={{ transition: 'all 0.5s ease' }}
          />
          <circle cx={cx} cy={cy} r="4" fill={main} stroke="#0a0a0f" strokeWidth="2" />
        </svg>
      </div>
      <div className="text-center -mt-1">
        <div className="text-2xl font-bold text-white" style={{ textShadow: `0 0 10px ${glow}` }}>
          {clampedScore.toFixed(1)}
        </div>
        <div className="text-[10px] font-bold tracking-widest" style={{ color: main }}>
          {label}
        </div>
      </div>
    </div>
  );
}

// Helper to describe SVG arc
function polarToCartesian(cx: number, cy: number, r: number, angleDeg: number) {
  const angleRad = (angleDeg * Math.PI) / 180;
  return { x: cx + r * Math.cos(angleRad), y: cy + r * Math.sin(angleRad) };
}

function describeArc(cx: number, cy: number, r: number, startAngle: number, endAngle: number) {
  const start = polarToCartesian(cx, cy, r, endAngle);
  const end = polarToCartesian(cx, cy, r, startAngle);
  const largeArc = endAngle - startAngle <= 180 ? '0' : '1';
  return `M ${start.x} ${start.y} A ${r} ${r} 0 ${largeArc} 0 ${end.x} ${end.y}`;
}
