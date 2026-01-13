/**
 * Type declarations for electrobun
 *
 * The electrobun package ships raw .ts files in dist/ which causes TypeScript
 * to type-check them as source files (rather than declarations). Since the
 * package has internal type errors, we provide our own declarations.
 */

declare module "electrobun/bun" {
  // RPC Schema type used to define request/response and message types
  export interface RPCSchema<T extends { requests?: object; messages?: object } = { requests: object; messages: object }> {
    requests?: T["requests"];
    messages?: T["messages"];
  }

  export interface BrowserWindowOptions<T = unknown> {
    title?: string;
    url?: string;
    frame?: {
      width?: number;
      height?: number;
      x?: number;
      y?: number;
    };
    rpc?: T;
  }

  export class BrowserWindow<T = unknown> {
    constructor(options: BrowserWindowOptions<T>);
    id: number;
    webviewId: number;
  }

  export interface BrowserViewOptions {
    frame?: {
      x?: number;
      y?: number;
      width?: number;
      height?: number;
    };
  }

  // Generic type for RPC handlers - uses `any` for params to allow typed handlers
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  export type RPCRequestHandler = (params: any) => Promise<any>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  export type RPCMessageHandler = (params: any) => void;

  export interface RPCHandlerConfig<T = unknown> {
    maxRequestTime?: number;
    handlers?: {
      requests?: Record<string, RPCRequestHandler>;
      messages?: Record<string, RPCMessageHandler>;
    };
  }

  export class BrowserView {
    static defineRPC<T>(options: RPCHandlerConfig<T>): T;
  }

  export interface ApplicationMenuItemConfig {
    label?: string;
    type?: "separator" | "divider";
    role?:
      | "quit"
      | "undo"
      | "redo"
      | "cut"
      | "copy"
      | "paste"
      | "selectAll"
      | "reload"
      | "toggleDevTools"
      | "zoomIn"
      | "zoomOut"
      | "resetZoom";
    accelerator?: string;
    action?: string;
    data?: unknown;
    enabled?: boolean;
    checked?: boolean;
    hidden?: boolean;
    tooltip?: string;
    submenu?: ApplicationMenuItemConfig[];
  }

  export interface ApplicationMenuConfig {
    label?: string;
    submenu?: ApplicationMenuItemConfig[];
  }

  export class ApplicationMenu {
    static setApplicationMenu(menu: ApplicationMenuConfig[]): void;
    static on(
      event: "application-menu-clicked",
      handler: (event: { id: number; action: string; data?: unknown }) => void
    ): void;
  }
}

declare module "electrobun/view" {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  export type RPCRequestHandler = (params: any) => Promise<any>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  export type RPCMessageHandler = (params: any) => void;

  export interface ElectroviewRPCOptions<T = unknown> {
    handlers?: {
      requests?: Record<string, RPCRequestHandler>;
      messages?: Record<string, RPCMessageHandler>;
    };
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  export interface ElectroviewRPC<T = any> {
    request: {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      [K: string]: (params: any) => Promise<any>;
    };
    send: {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      [K: string]: (params: any) => void;
    };
  }

  export class Electroview {
    static defineRPC<T>(options: ElectroviewRPCOptions<T>): ElectroviewRPC<T>;
  }
}
