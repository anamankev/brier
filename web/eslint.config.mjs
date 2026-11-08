import next from "eslint-config-next";

const config = [
  ...next,
  { ignores: [".next/", "node_modules/", "playwright-report/", "test-results/"] },
];

export default config;
