import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./src/**/*.{js,ts,jsx,tsx,mdx}",
    "./node_modules/@playsistemico/modo-bo-ui-lib/dist/**/*.js",
  ],
  theme: {
    extend: {},
  },
  plugins: [],
};

export default config;
