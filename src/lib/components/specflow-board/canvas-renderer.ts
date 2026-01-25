/**
 * Canvas renderer for SpecFlow board
 * Renders the grid background and dependency wires
 */

import type { Island, Transform, Wire } from './types';

export class CanvasRenderer {
  private ctx: CanvasRenderingContext2D;
  private dpr: number;
  private width = 0;
  private height = 0;

  constructor(canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Failed to get 2d context');
    this.ctx = ctx;
    this.dpr = window.devicePixelRatio || 1;
  }

  resize(width: number, height: number) {
    this.width = width;
    this.height = height;
  }

  render(
    transform: Transform,
    wires: Wire[],
    islands: Island[],
    viewportWidth: number,
    viewportHeight: number,
  ) {
    const { ctx, dpr } = this;
    const { x, y, k } = transform;

    // Clear
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, viewportWidth * dpr, viewportHeight * dpr);

    // Apply transform
    ctx.setTransform(dpr * k, 0, 0, dpr * k, dpr * x, dpr * y);

    // Draw grid
    this.drawGrid(transform, viewportWidth, viewportHeight);

    // Draw wires
    this.drawWires(wires, islands);
  }

  private drawGrid(transform: Transform, vw: number, vh: number) {
    const { ctx } = this;
    const { x, y, k } = transform;

    // Grid spacing based on zoom
    const majorSpacing = 100;
    const minorSpacing = 20;
    const showMinor = k >= 0.3;

    // Calculate visible world bounds
    const worldLeft = Math.floor(-x / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldTop = Math.floor(-y / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldRight = Math.ceil((-x + vw) / k / majorSpacing) * majorSpacing + majorSpacing;
    const worldBottom = Math.ceil((-y + vh) / k / majorSpacing) * majorSpacing + majorSpacing;

    // Minor grid (dots)
    if (showMinor) {
      ctx.fillStyle = 'rgba(255, 255, 255, 0.05)';
      for (let wx = worldLeft; wx <= worldRight; wx += minorSpacing) {
        for (let wy = worldTop; wy <= worldBottom; wy += minorSpacing) {
          ctx.beginPath();
          ctx.arc(wx, wy, 1 / k, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    // Major grid (dots)
    ctx.fillStyle = 'rgba(255, 255, 255, 0.1)';
    for (let wx = worldLeft; wx <= worldRight; wx += majorSpacing) {
      for (let wy = worldTop; wy <= worldBottom; wy += majorSpacing) {
        ctx.beginPath();
        ctx.arc(wx, wy, 2 / k, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }

  private drawWires(wires: Wire[], islands: Island[]) {
    const { ctx } = this;
    const islandMap = new Map(islands.map((i) => [i.id, i]));

    ctx.strokeStyle = 'rgba(245, 158, 11, 0.5)'; // amber-500
    ctx.lineWidth = 2;

    for (const wire of wires) {
      const from = islandMap.get(wire.fromIslandId);
      const to = islandMap.get(wire.toIslandId);
      if (!from || !to) continue;

      // Calculate connection points (right side of from, left side of to)
      const fromX = from.x + from.width;
      const fromY = from.y + 50; // Header height
      const toX = to.x;
      const toY = to.y + 50;

      // Draw bezier curve
      const cp1x = fromX + (toX - fromX) * 0.5;
      const cp1y = fromY;
      const cp2x = fromX + (toX - fromX) * 0.5;
      const cp2y = toY;

      ctx.beginPath();
      ctx.moveTo(fromX, fromY);
      ctx.bezierCurveTo(cp1x, cp1y, cp2x, cp2y, toX, toY);
      ctx.stroke();

      // Arrowhead
      const angle = Math.atan2(toY - cp2y, toX - cp2x);
      const arrowSize = 8;
      ctx.beginPath();
      ctx.moveTo(toX, toY);
      ctx.lineTo(
        toX - arrowSize * Math.cos(angle - Math.PI / 6),
        toY - arrowSize * Math.sin(angle - Math.PI / 6),
      );
      ctx.lineTo(
        toX - arrowSize * Math.cos(angle + Math.PI / 6),
        toY - arrowSize * Math.sin(angle + Math.PI / 6),
      );
      ctx.closePath();
      ctx.fillStyle = 'rgba(245, 158, 11, 0.5)';
      ctx.fill();
    }
  }
}
