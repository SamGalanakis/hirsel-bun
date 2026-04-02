/** @type {import('tailwindcss').Config} */
module.exports = {
  darkMode: ["class", "[data-kb-theme='dark']"],
  content: ["./src/**/*.{ts,tsx}", "./index.html"],
  theme: {
    borderRadius: {
      none: "0",
      DEFAULT: "0",
      sm: "0",
      md: "0",
      lg: "0",
      xl: "0",
      "2xl": "0",
      full: "9999px",
    },
    screens: {
      sm: "640px",
      md: "900px",
      lg: "1180px",
    },
    extend: {
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
        signal: {
          red: "hsl(var(--signal-red))",
          blue: "hsl(var(--signal-blue))",
          amber: "hsl(var(--signal-amber))",
          green: "hsl(var(--signal-green))",
        },
        ink: {
          DEFAULT: "hsl(var(--foreground))",
          2: "hsl(var(--ink-2))",
          3: "hsl(var(--muted-foreground))",
        },
      },
      fontFamily: {
        display: ['"Bricolage Grotesque"', "serif"],
        body: ['"Outfit"', "system-ui", "sans-serif"],
        mono: ['"Martian Mono"', "ui-monospace", "monospace"],
      },
      borderWidth: {
        DEFAULT: "1.5px",
        0: "0",
        1: "1px",
        2: "2px",
        4: "4px",
      },
      boxShadow: {
        sm: "var(--shadow-sm)",
        md: "var(--shadow-md)",
        lift: "var(--shadow-lift)",
      },
      keyframes: {
        "pulse-dot": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.4" },
        },
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--kb-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--kb-accordion-content-height)" },
          to: { height: "0" },
        },
      },
      animation: {
        "pulse-dot": "pulse-dot 1.5s ease-in-out infinite",
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};
