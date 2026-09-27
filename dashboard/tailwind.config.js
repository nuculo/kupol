/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        background: '#0a0a0f',
        panel: 'rgba(255, 255, 255, 0.03)',
        borderBase: 'rgba(255, 255, 255, 0.1)',
        neonGreen: '#39ff14',
        neonRed: '#ff073a',
        neonBlue: '#00f3ff',
      },
      animation: {
        'pulse-slow': 'pulse 4s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        'glow': 'glow 2s ease-in-out infinite alternate',
      },
      keyframes: {
        glow: {
          '0%': { boxShadow: '0 0 5px rgba(57, 255, 20, 0.2), 0 0 20px rgba(57, 255, 20, 0.2)' },
          '100%': { boxShadow: '0 0 10px rgba(57, 255, 20, 0.6), 0 0 40px rgba(57, 255, 20, 0.4)' },
        }
      }
    },
  },
  plugins: [],
}
