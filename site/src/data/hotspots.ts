// Measured positions on the screenshots, as fractions of the image (0..1), so they survive a re-capture at
// a different scale as long as the window layout is the same.

/** Targets of the three callouts on the main window screenshot (440 x 680 logical, Advanced open, HDR on):
 * Identify button, SDR dropdown, Peak brightness value. */
export const MAIN_CALLOUTS: ReadonlyArray<{ x: number; y: number }> = [
  { x: 420 / 440, y: 36 / 680 },
  { x: 420 / 440, y: 160 / 680 },
  { x: 420 / 440, y: 409 / 680 },
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
