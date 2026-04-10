import type { Config } from 'tailwindcss';

export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        brand: {
          50: '#edf5ff',
          100: '#d0e3ff',
          200: '#a3caff',
          300: '#75b0ff',
          400: '#4897ff',
          500: '#1b7dff',
          600: '#005ee6',
          700: '#0047b4',
          800: '#003281',
          900: '#001e4f'
        }
      }
    }
  },
  plugins: []
} satisfies Config;
