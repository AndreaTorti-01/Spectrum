// Ported from STANDARD_SPACES in src/color.rs (names, primaries, colors and descriptions verbatim).
// Order: srgb, display-p3, adobe-rgb, prophoto, rec2020, dci-p3.

export interface Pt {
  x: number;
  y: number;
}
export interface Prims {
  r: Pt;
  g: Pt;
  b: Pt;
  w: Pt;
}
export interface Space {
  id: string;
  name: string;
  prims: Prims;
  description: string;
  color: string;
}

const p = (x: number, y: number): Pt => ({ x, y });
export const D65: Pt = p(0.3127, 0.329);

export const BT709: Prims = { r: p(0.64, 0.33), g: p(0.3, 0.6), b: p(0.15, 0.06), w: D65 };
export const BT2020: Prims = { r: p(0.708, 0.292), g: p(0.17, 0.797), b: p(0.131, 0.046), w: D65 };

export const SPACES: readonly Space[] = [
  {
    id: "srgb",
    name: "sRGB",
    prims: BT709,
    description: "The default for the web and Windows. A monitor's sRGB mode emulates it.",
    color: "#8FA3B8",
  },
  {
    id: "display-p3",
    name: "Display P3",
    prims: { r: p(0.68, 0.32), g: p(0.265, 0.69), b: p(0.15, 0.06), w: D65 },
    description: "DCI-P3 primaries with a D65 white and sRGB curve, used by Apple displays and many OLED monitors.",
    color: "#2EC4B6",
  },
  {
    id: "adobe-rgb",
    name: "Adobe RGB (1998)",
    prims: { r: p(0.64, 0.33), g: p(0.21, 0.71), b: p(0.15, 0.06), w: D65 },
    description: "Print and photography space with wider greens and cyans than sRGB and the same red and blue.",
    color: "#FFC93C",
  },
  {
    id: "prophoto",
    name: "ProPhoto RGB",
    prims: { r: p(0.7347, 0.2653), g: p(0.1596, 0.8404), b: p(0.0366, 0.0001), w: p(0.3457, 0.3585) },
    description: "Editing space for RAW photos whose green and blue primaries fall outside visible color.",
    color: "#F28AB2",
  },
  {
    id: "rec2020",
    name: "Rec. 2020",
    prims: BT2020,
    description: "Gamut of UHD and HDR video, with primaries at 630, 532 and 467 nm.",
    color: "#9BD94A",
  },
  {
    id: "dci-p3",
    name: "DCI-P3",
    prims: { r: p(0.68, 0.32), g: p(0.265, 0.69), b: p(0.15, 0.06), w: p(0.314, 0.351) },
    description: "Digital cinema standard with Display P3 primaries, a greenish projector white and gamma 2.6.",
    color: "#FF5A3C",
  },
];

export const SPACE_BY_ID: Record<string, Space> = Object.fromEntries(SPACES.map((s) => [s.id, s]));
