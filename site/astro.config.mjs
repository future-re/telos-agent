import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://future-re.github.io',
  base: '/telos-agent',
  integrations: [
    starlight({
      title: 'telos',
      description: 'Rust agent runtime for tool execution, CLI workflows, MCP, plugins, and product clients.',
      favicon: '/telos-agent/favicon.svg',
      customCss: ['./src/styles/global.css'],
      social: {
        github: 'https://github.com/future-re/telos-agent',
      },
      sidebar: [
        {
          label: 'Start',
          items: [
            { label: 'Introduction', slug: 'introduction' },
            { label: 'Quick Start', slug: 'quick-start' },
            { label: 'Core Concepts', slug: 'core-concepts' },
          ],
        },
        {
          label: 'Use telos',
          items: [
            { label: 'Library API Guide', slug: 'library-api' },
            { label: 'CLI Guide', slug: 'cli-guide' },
            { label: 'Desktop Client', slug: 'desktop-client' },
            { label: 'Configuration', slug: 'configuration' },
          ],
        },
        {
          label: 'Extend',
          items: [
            { label: 'Plugins and MCP', slug: 'plugins-mcp' },
            { label: 'Deployment', slug: 'deployment' },
            { label: 'Rust API Reference', slug: 'rust-api-reference' },
          ],
        },
      ],
    }),
  ],
});
