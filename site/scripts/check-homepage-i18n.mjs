import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const page = readFileSync(join(scriptDir, '../src/pages/index.astro'), 'utf8');

const checks = [
  ['language switch control', 'class="language-switch"'],
  ['Chinese language label', '中文'],
  ['English homepage copy', 'Let agents call tools safely'],
  ['Chinese homepage copy', '让智能体安全调用工具'],
  ['client-side language script', 'data-language-target'],
];

const missing = checks.filter(([, token]) => !page.includes(token));

if (missing.length > 0) {
  console.error('Homepage i18n check failed. Missing:');
  for (const [label] of missing) {
    console.error(`- ${label}`);
  }
  process.exit(1);
}

console.log('Homepage i18n check passed.');
