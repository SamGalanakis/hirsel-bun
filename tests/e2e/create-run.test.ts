/**
 * E2E test: Create a new run
 *
 * This test demonstrates how to use the MCP test client to automate
 * the Hirsel UI for creating a new run.
 *
 * Prerequisites:
 * - Hirsel must be running with the MCP server enabled
 * - Run with: npx tsx tests/e2e/create-run.test.ts
 */

import { McpTestClient } from '../mcp-client.js';
import * as path from 'node:path';
import * as fs from 'node:fs';

const SCREENSHOT_DIR = path.join(__dirname, '..', 'screenshots');

async function ensureScreenshotDir(): Promise<void> {
  if (!fs.existsSync(SCREENSHOT_DIR)) {
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
  }
}

async function test(): Promise<void> {
  const client = new McpTestClient();

  try {
    console.log('Connecting to MCP server...');
    await client.connect();

    // Verify connection
    const pong = await client.ping();
    if (!pong) {
      throw new Error('MCP server ping failed');
    }
    console.log('Connected to MCP server');

    // Take initial screenshot
    await ensureScreenshotDir();
    await client.screenshot(path.join(SCREENSHOT_DIR, '01-initial.png'));
    console.log('Took initial screenshot');

    // Click the new run button
    console.log('Clicking new run button...');
    await client.click('new-run-btn');

    // Wait for the spec input to appear
    console.log('Waiting for spec input...');
    await client.waitFor('spec-input', 10000);

    // Take screenshot of draft editor
    await client.screenshot(path.join(SCREENSHOT_DIR, '02-draft-editor.png'));
    console.log('Took draft editor screenshot');

    // Type spec content
    console.log('Typing spec...');
    await client.type(
      'spec-input',
      `# Hello World

Build a simple "Hello World" application.

## Requirements
- Print "Hello, World!" to stdout
- Exit with code 0
`,
      5
    );

    // Take screenshot after typing
    await client.screenshot(path.join(SCREENSHOT_DIR, '03-spec-entered.png'));
    console.log('Took spec entered screenshot');

    // Check if start button exists (may need to select starting point first)
    const startBtnExists = await client.exists('start-run-btn');
    if (startBtnExists) {
      console.log('Start button found');
      // Note: We don't actually click start as it would create a real run
      // In a real test, you would:
      // await client.click('start-run-btn');
      // await client.waitFor('run-item-...', 30000);
    } else {
      console.log('Start button not visible (may need to select starting point)');
    }

    // Take final screenshot
    await client.screenshot(path.join(SCREENSHOT_DIR, '04-final.png'));
    console.log('Took final screenshot');

    console.log('\nTest completed successfully!');
    console.log(`Screenshots saved to: ${SCREENSHOT_DIR}`);
  } catch (error) {
    console.error('Test failed:', error);

    // Take failure screenshot
    try {
      await ensureScreenshotDir();
      const timestamp = Date.now();
      await client.screenshot(path.join(SCREENSHOT_DIR, `FAILED-${timestamp}.png`));
      console.log(`Failure screenshot saved`);
    } catch {
      console.error('Failed to take failure screenshot');
    }

    process.exit(1);
  } finally {
    await client.disconnect();
  }
}

// Run the test
test();
