// Measured positions on the screenshots, as fractions of the image (0..1), so they survive a re-capture at
// a different scale as long as the window layout is the same.

/** Targets of the three callouts on the main window screenshot: monitor tile, SDR dropdown, HDR slider. */
export const MAIN_CALLOUTS: ReadonlyArray<{ x: number; y: number }> = [
  { x: 0.615, y: 0.104 },
  { x: 0.953, y: 0.267 },
  { x: 0.976, y: 0.65 },
];

/** Plot area of the gamut window screenshot: data (x, y) to fractions of the image. */
export const GAMUT_PLOT = {
  x0: 172 / 1780,
  sx: 1198.75 / 1780,
  y0: 1160 / 1220,
  sy: 1198.9 / 1220,
  /** image aspect (w / h) the numbers above were measured on */
  aspect: 1780 / 1220,
};
