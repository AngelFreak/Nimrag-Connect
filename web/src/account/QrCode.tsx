import { useMemo } from 'react';
import qrcode from 'qrcode-generator';

/** One `<path>` of unit squares, merged per row, and the side length with its quiet zone. */
export function qrPath(value: string): { size: number; path: string } {
  const qr = qrcode(0, 'M');
  qr.addData(value);
  qr.make();
  const n = qr.getModuleCount();
  const quiet = 4;
  let path = '';
  for (let row = 0; row < n; row++) {
    let col = 0;
    while (col < n) {
      if (!qr.isDark(row, col)) {
        col++;
        continue;
      }
      const start = col;
      while (col < n && qr.isDark(row, col)) col++;
      path += `M${start + quiet} ${row + quiet}h${col - start}v1h${start - col}z`;
    }
  }
  return { size: n + quiet * 2, path };
}

/**
 * A QR code drawn in the browser as inline SVG. The text never leaves the page: no canvas, no
 * image URL, no request. Scanners need dark modules on a light ground, so the colours are fixed
 * and do not follow the theme. The accessible name stands in for the picture; the caller shows
 * the same secret as text for manual entry.
 */
export function QrCode({ value, label }: { value: string; label: string }) {
  const { size, path } = useMemo(() => qrPath(value), [value]);
  return (
    <svg className="qr" role="img" aria-label={label} viewBox={`0 0 ${size} ${size}`} shapeRendering="crispEdges">
      <rect width={size} height={size} fill="#ffffff" />
      <path d={path} fill="#000000" />
    </svg>
  );
}
