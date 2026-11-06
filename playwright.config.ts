import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests/ui',
  use: { viewport: { width: 1280, height: 900 } },
  webServer: { command: 'npm run dev', url: 'http://127.0.0.1:1420', reuseExistingServer: !process.env.CI },
});
