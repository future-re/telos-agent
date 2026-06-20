import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const page = readFileSync(join(scriptDir, '../src/pages/index.astro'), 'utf8');
const styles = readFileSync(join(scriptDir, '../src/styles/global.css'), 'utf8');

const checks = [
  ['language switch control', 'class="language-switch"'],
  ['Chinese language label', '中文'],
  ['English product headline', 'Controlled autonomy for tool-using agents'],
  ['Chinese product headline', '面向工具型智能体的受控自主运行时'],
  ['runtime visual', 'runtime-map'],
  ['approval step copy', 'Approval gate'],
  ['Cargo CLI install', 'cargo install telos-cli'],
  ['PyPI CLI install', 'pip install telos-cli'],
  ['TUI start command', 'telos\ntelos chat'],
  ['client-side language script', 'data-language-target'],
  ['Quick Start link', "/docs/quick-start/"],
  ['Rust API link', "/api/rust/telos_agent/"],
];

const missing = checks.filter(([, token]) => !page.includes(token));

if (missing.length > 0) {
  console.error('Homepage i18n check failed. Missing:');
  for (const [label] of missing) {
    console.error(`- ${label}`);
  }
  process.exit(1);
}

const styleChecks = [
  [
    'resilient English hiding rule',
    /\[data-lang='zh'\]\s+\[data-lang-copy='en'\][^{]*\{[^}]*display:\s*none\s*!important/i,
  ],
  [
    'resilient Chinese hiding rule',
    /\[data-lang='en'\]\s+\[data-lang-copy='zh'\][^{]*\{[^}]*display:\s*none\s*!important/i,
  ],
];

const missingStyleChecks = styleChecks.filter(([, pattern]) => !pattern.test(styles));

if (missingStyleChecks.length > 0) {
  console.error('Homepage i18n CSS check failed. Missing:');
  for (const [label] of missingStyleChecks) {
    console.error(`- ${label}`);
  }
  process.exit(1);
}

const distDir = join(scriptDir, '../dist');
const builtRouteChecks = [
  ['Quick Start route', 'docs/quick-start/index.html'],
  ['CLI Guide route', 'docs/cli-guide/index.html'],
  ['Configuration route', 'docs/configuration/index.html'],
  ['Plugins and MCP route', 'docs/plugins-mcp/index.html'],
];

if (existsSync(distDir)) {
  const missingRoutes = builtRouteChecks.filter(([, route]) => !existsSync(join(distDir, route)));

  if (missingRoutes.length > 0) {
    console.error('Homepage built-route check failed. Missing:');
    for (const [label, route] of missingRoutes) {
      console.error(`- ${label}: dist/${route}`);
    }
    process.exit(1);
  }
}

console.log('Homepage i18n check passed.');
