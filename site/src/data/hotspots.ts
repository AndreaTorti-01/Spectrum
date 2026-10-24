// Positions on the screenshots, as fractions of the image (0..1), so they hold at any scale. The numbers
// come from the app's layout code: `Spectrum.exe --render layout` writes layout.json (scripts/render.ps1).
import layout from "./layout.json" with { type: "json" };

const { main, gamut } = layout;

/** Targets of the three callouts on the main window screenshot (Advanced open, HDR on): the first monitor's
 * tile, SDR dropdown, Read from monitor. */
export const MAIN_CALLOUTS: ReadonlyArray<{ x: number; y: number }> = main.callouts.map((c) => ({
  x: c.x / main.width,
  y: c.y / main.height,
}));

const { plot, domain } = gamut;
const plotW = plot.right - plot.left;
const plotH = plot.bottom - plot.top;

/** Plot area of the gamut window screenshot: data (x, y) to fractions of the image. */
export const GAMUT_PLOT = {
  x0: (plot.left - (domain.xMin * plotW) / (domain.xMax - domain.xMin)) / gamut.width,
  sx: plotW / (domain.xMax - domain.xMin) / gamut.width,
  y0: plot.bottom / gamut.height,
  sy: plotH / domain.yMax / gamut.height,
  /** image aspect (w / h) the numbers above apply to */
  aspect: gamut.width / gamut.height,
};
