import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://future-re.github.io',
  base: '/telos-agent',
  integrations: [
    starlight({
      title: 'telos',
      description: 'Rust agent runtime for tool execution, CLI workflows, MCP, plugins, and product clients.',
      favicon: '/favicon.svg',
      customCss: ['./src/styles/global.css'],
      social: {
        github: 'https://github.com/future-re/telos-agent',
      },
      sidebar: [
        {
          label: 'Start',
          items: [
            { label: 'Introduction', slug: 'docs/introduction' },
            { label: 'Quick Start', slug: 'docs/quick-start' },
            { label: 'Core Concepts', slug: 'docs/core-concepts' },
          ],
        },
        {
          label: 'Use telos',
          items: [
            { label: 'Library API Guide', slug: 'docs/library-api' },
            { label: 'CLI Guide', slug: 'docs/cli-guide' },
            { label: 'Desktop Client', slug: 'docs/desktop-client' },
            { label: 'Configuration', slug: 'docs/configuration' },
          ],
        },
        {
          label: 'Extend',
          items: [
            { label: 'Plugins and MCP', slug: 'docs/plugins-mcp' },
            { label: 'Deployment', slug: 'docs/deployment' },
            { label: 'Rust API Reference', slug: 'docs/rust-api-reference' },
          ],
        },
      ],
    }),
  ],
});
