/**
 * MCP Test Client for Hirsel UI automation
 *
 * Connects to the tauri-mcp socket and provides helpers for UI testing.
 */

import * as net from 'node:net';
import * as fs from 'node:fs';
import * as path from 'node:path';

/** Socket response from the MCP server */
interface SocketResponse {
  success: boolean;
  data?: unknown;
  error?: string;
}

/** Element position data returned by get_element_position */
interface ElementPosition {
  x: number;
  y: number;
  width: number;
  height: number;
  top: number;
  left: number;
  clicked?: boolean;
}

/**
 * MCP Test Client for automating Hirsel UI tests.
 *
 * Example usage:
 * ```ts
 * const client = new McpTestClient();
 * await client.connect();
 * await client.click('new-run-btn');
 * await client.type('spec-input', 'Build a hello world app');
 * await client.click('start-run-btn');
 * await client.disconnect();
 * ```
 */
export class McpTestClient {
  private socket: net.Socket | null = null;
  private socketPath: string;
  private responseBuffer = '';
  private pendingRequests: Map<
    number,
    { resolve: (value: SocketResponse) => void; reject: (error: Error) => void }
  > = new Map();
  private requestId = 0;

  constructor(socketPath?: string) {
    this.socketPath = socketPath || '/tmp/tauri-mcp.sock';
  }

  /**
   * Connect to the MCP socket server.
   */
  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.socket = net.createConnection({ path: this.socketPath }, () => {
        resolve();
      });

      this.socket.on('data', (data) => {
        this.handleData(data.toString());
      });

      this.socket.on('error', (err) => {
        reject(err);
      });

      this.socket.on('close', () => {
        this.socket = null;
      });
    });
  }

  /**
   * Disconnect from the MCP socket server.
   */
  async disconnect(): Promise<void> {
    return new Promise((resolve) => {
      if (this.socket) {
        this.socket.end(() => {
          this.socket = null;
          resolve();
        });
      } else {
        resolve();
      }
    });
  }

  /**
   * Handle incoming data from the socket.
   */
  private handleData(data: string): void {
    this.responseBuffer += data;

    // Process complete lines (newline-delimited JSON)
    let newlineIndex: number;
    while ((newlineIndex = this.responseBuffer.indexOf('\n')) !== -1) {
      const line = this.responseBuffer.slice(0, newlineIndex);
      this.responseBuffer = this.responseBuffer.slice(newlineIndex + 1);

      if (line.trim()) {
        try {
          const response = JSON.parse(line) as SocketResponse;
          // For simplicity, resolve the oldest pending request
          const oldest = this.pendingRequests.entries().next().value;
          if (oldest) {
            const [id, { resolve }] = oldest;
            this.pendingRequests.delete(id);
            resolve(response);
          }
        } catch {
          console.error('Failed to parse response:', line);
        }
      }
    }
  }

  /**
   * Send a command to the MCP server and wait for response.
   */
  private async sendCommand(command: string, payload: unknown = {}): Promise<SocketResponse> {
    if (!this.socket) {
      throw new Error('Not connected to MCP server');
    }

    return new Promise((resolve, reject) => {
      const id = ++this.requestId;
      this.pendingRequests.set(id, { resolve, reject });

      const message = JSON.stringify({ command, payload }) + '\n';
      this.socket!.write(message, (err) => {
        if (err) {
          this.pendingRequests.delete(id);
          reject(err);
        }
      });

      // Timeout after 30 seconds
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error(`Command ${command} timed out`));
        }
      }, 30000);
    });
  }

  /**
   * Execute JavaScript in the webview.
   */
  async executeJs(code: string): Promise<unknown> {
    const response = await this.sendCommand('execute_js', {
      window_label: 'main',
      code,
    });

    if (!response.success) {
      throw new Error(response.error || 'executeJs failed');
    }

    return response.data;
  }

  /**
   * Click an element by data-test attribute.
   */
  async click(dataTestValue: string): Promise<void> {
    const response = await this.sendCommand('get_element_position', {
      window_label: 'main',
      selector_type: 'data-test',
      selector_value: dataTestValue,
      should_click: true,
      raw_coordinates: false,
    });

    if (!response.success) {
      throw new Error(response.error || `Failed to click element: ${dataTestValue}`);
    }
  }

  /**
   * Type text into an element by data-test attribute.
   */
  async type(dataTestValue: string, text: string, delayMs = 20): Promise<void> {
    const response = await this.sendCommand('send_text_to_element', {
      window_label: 'main',
      selector_type: 'data-test',
      selector_value: dataTestValue,
      text,
      delay_ms: delayMs,
    });

    if (!response.success) {
      throw new Error(response.error || `Failed to type into element: ${dataTestValue}`);
    }
  }

  /**
   * Get text content of an element by data-test attribute.
   */
  async getText(dataTestValue: string): Promise<string> {
    const result = await this.executeJs(`
      const el = document.querySelector('[data-test="${dataTestValue}"]');
      el ? el.textContent || el.value || '' : null;
    `);

    if (result === null) {
      throw new Error(`Element not found: ${dataTestValue}`);
    }

    return result as string;
  }

  /**
   * Wait for an element to appear by data-test attribute.
   */
  async waitFor(dataTestValue: string, timeoutMs = 5000): Promise<void> {
    const startTime = Date.now();

    while (Date.now() - startTime < timeoutMs) {
      const response = await this.sendCommand('get_element_position', {
        window_label: 'main',
        selector_type: 'data-test',
        selector_value: dataTestValue,
        should_click: false,
        raw_coordinates: false,
      });

      if (response.success) {
        return;
      }

      // Wait 100ms before retrying
      await new Promise((resolve) => setTimeout(resolve, 100));
    }

    throw new Error(`Timeout waiting for element: ${dataTestValue}`);
  }

  /**
   * Check if an element exists by data-test attribute.
   */
  async exists(dataTestValue: string): Promise<boolean> {
    const response = await this.sendCommand('get_element_position', {
      window_label: 'main',
      selector_type: 'data-test',
      selector_value: dataTestValue,
      should_click: false,
      raw_coordinates: false,
    });

    return response.success;
  }

  /**
   * Get element position by data-test attribute.
   */
  async getPosition(dataTestValue: string): Promise<ElementPosition> {
    const response = await this.sendCommand('get_element_position', {
      window_label: 'main',
      selector_type: 'data-test',
      selector_value: dataTestValue,
      should_click: false,
      raw_coordinates: false,
    });

    if (!response.success) {
      throw new Error(response.error || `Element not found: ${dataTestValue}`);
    }

    return response.data as ElementPosition;
  }

  /**
   * Take a screenshot and save it to a file.
   */
  async screenshot(filePath: string): Promise<void> {
    const response = await this.sendCommand('take_screenshot', {
      window_label: 'main',
    });

    if (!response.success) {
      throw new Error(response.error || 'Failed to take screenshot');
    }

    // Response data contains base64-encoded image with data URL prefix
    const data = response.data as { data: string; mime_type: string };
    if (!data?.data) {
      throw new Error('Screenshot data is empty');
    }

    // Remove data URL prefix (e.g., "data:image/png;base64,")
    const base64Data = data.data.replace(/^data:image\/\w+;base64,/, '');
    const buffer = Buffer.from(base64Data, 'base64');

    // Ensure directory exists
    const dir = path.dirname(filePath);
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }

    fs.writeFileSync(filePath, buffer);
  }

  /**
   * Get a screenshot as a Buffer.
   */
  async getScreenshot(): Promise<Buffer> {
    const response = await this.sendCommand('take_screenshot', {
      window_label: 'main',
    });

    if (!response.success) {
      throw new Error(response.error || 'Failed to take screenshot');
    }

    const data = response.data as { data: string; mime_type: string };
    if (!data?.data) {
      throw new Error('Screenshot data is empty');
    }

    const base64Data = data.data.replace(/^data:image\/\w+;base64,/, '');
    return Buffer.from(base64Data, 'base64');
  }

  /**
   * Ping the MCP server to check if it's alive.
   */
  async ping(): Promise<boolean> {
    try {
      const response = await this.sendCommand('ping', {});
      return response.success;
    } catch {
      return false;
    }
  }

  /**
   * Get the DOM content of the webview.
   */
  async getDom(): Promise<string> {
    const response = await this.sendCommand('get_dom', {
      window_label: 'main',
    });

    if (!response.success) {
      throw new Error(response.error || 'Failed to get DOM');
    }

    return response.data as string;
  }
}

// Export singleton instance for convenience
export const mcpClient = new McpTestClient();
