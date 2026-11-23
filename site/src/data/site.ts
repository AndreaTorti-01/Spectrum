// The one place that names the repository and the site. build/html.ts
// (a Vite transformIndexHtml plugin) fills %REPO%, %STORE%, %LICENSE% and %SITE_URL% in index.html
// (links, canonical, og:url, og:image, twitter:image) from these constants.
export const REPO = "https://github.com/AndreaTorti-01/Spectrum";
export const SITE_URL = "https://andreatorti-01.github.io/Spectrum/";
// Partner Center > Product management > Product identity > Store ID (12 characters, 9N... or 9P...).
export const STORE_ID = "STORE_ID";
// The Store listing; mode=direct opens the Microsoft Store app instead of the web page where it can.
export const STORE = "https://apps.microsoft.com/detail/" + STORE_ID + "?mode=direct";
export const LICENSE_URL = REPO + "/blob/main/LICENSE";

/** Measured values, see the measurement report. */
