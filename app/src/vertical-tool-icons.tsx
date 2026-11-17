import type { ReactNode } from "react";

// Solid silhouettes for the vertical toolbar. currentColor keeps them legible
// in light, dark, high contrast and selected states.
const glyphs: Record<string, ReactNode> = {
  select: <path d="M5 2.8 19 12l-6.2.5 2.8 6.4-2.5 1.1-2.8-6.3L5 18z" fill="currentColor" stroke="none" />,
  line: (
    <>
      <path d="m6 14 10-10 4 4L10 18 4 20z" fill="currentColor" fillOpacity=".2" />
      <path d="m14 6 4 4M6 14l4 4M5.1 17.6l1.3 1.3" />
    </>
  ),
  rect: (
    <>
      <path d="M4 6h16v12H4z" fill="currentColor" fillOpacity=".18" />
      <path d="M4 18 20 6" strokeDasharray="2 2.3" />
      <circle cx="4" cy="18" r="1.35" fill="currentColor" stroke="none" />
      <circle cx="20" cy="6" r="1.35" fill="currentColor" stroke="none" />
    </>
  ),
  push: (
    <>
      <path d="m3.5 13.2 8.5-3.5 8.5 3.5v4.3L12 21l-8.5-3.5z" fill="currentColor" fillOpacity=".2" />
      <path d="m3.5 13.2 8.5 3.5 8.5-3.5M12 16.7V21" />
      <path d="M11 12V6H8l4-4.5L16 6h-3v6z" fill="currentColor" stroke="none" />
    </>
  ),
  move: <path d="M12 2 8 6h2.8v4.8H6V8l-4 4 4 4v-2.8h4.8V18H8l4 4 4-4h-2.8v-4.8H18V16l4-4-4-4v2.8h-4.8V6H16z" fill="currentColor" stroke="none" />,
  rotate: (
    <>
      <path d="M3.5 17a8.5 8.5 0 0 1 17 0z" fill="currentColor" fillOpacity=".18" />
      <path d="M12 17v-3m-5.6-3.2 1.4 1.6m9.8-1.6-1.4 1.6M12 5.3V2.5m0 0-2.3 2.3m2.3-2.3 2.3 2.3" />
      <circle cx="12" cy="17" r="1.25" fill="currentColor" stroke="none" />
    </>
  ),
  tape: (
    <>
      <path d="M4 9a4 4 0 0 1 4-4h3a3 3 0 0 1 3 3v9H7a3 3 0 0 1-3-3z" fill="currentColor" fillOpacity=".2" />
      <path d="M14 13h6.5v4H14m2.1-4v1.5m2.2-1.5v1.5" />
      <circle cx="9" cy="11" r="1.6" />
    </>
  ),
  orbit: (
    <>
      <ellipse cx="12" cy="12" rx="9" ry="4.3" transform="rotate(-27 12 12)" />
      <path d="M8 4.7c2-1.4 5.4 1.1 7.6 5.5s2.2 8.9.3 10.1M5 13l-2.1 1.2 2.4.9m13.7-4.2 2.1-1.2-2.4-.9" />
      <circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none" />
    </>
  ),
};

export function verticalToolIcon(id: string) {
  if (!Object.hasOwn(glyphs, id)) return undefined;
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {glyphs[id]}
    </svg>
  );
}
