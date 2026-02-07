export interface ViewportPoint {
  x: number;
  y: number;
}

export function viewportCenterPoint(rect: DOMRect, treeLeftMargin: number): ViewportPoint {
  return { x: rect.width / 2 - treeLeftMargin, y: 0 };
}

// Y coordinates on the canvas are rendered with an extra translateY(-50%) in SpecBoard,
// so "content Y" is effectively centered around height/2. Convert absolute content Y
// (0..height) into a relative coordinate centered at 0.
export function contentYToRelative(contentY: number, contentHeight: number): number {
  return contentY - contentHeight / 2;
}

export function panForContentPoint(
  viewportPoint: ViewportPoint,
  contentPoint: ViewportPoint,
  zoom: number,
): ViewportPoint {
  return {
    x: viewportPoint.x - contentPoint.x * zoom,
    y: viewportPoint.y - contentPoint.y * zoom,
  };
}

export function zoomAroundViewportPoint(
  viewportPoint: ViewportPoint,
  oldZoom: number,
  newZoom: number,
  pan: ViewportPoint,
): ViewportPoint {
  const contentX = (viewportPoint.x - pan.x) / oldZoom;
  const contentY = (viewportPoint.y - pan.y) / oldZoom;
  return {
    x: viewportPoint.x - contentX * newZoom,
    y: viewportPoint.y - contentY * newZoom,
  };
}
