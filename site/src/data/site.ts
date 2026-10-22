// The one place that names the repository and the site. build/html.ts
// (a Vite transformIndexHtml plugin) fills %REPO%, %RELEASES%, %DOWNLOAD%, %LICENSE% and %SITE_URL% in index.html
// (links, canonical, og:url, og:image, twitter:image) from these constants.
export const REPO = "https://github.com/AndreaTorti-01/Spectrum";
export const SITE_URL = "https://andreatorti-01.github.io/Spectrum/";
export const RELEASES = REPO + "/releases/latest";
// GitHub redirects this to the asset of the newest release; the release workflow keeps the name.
export const DOWNLOAD = RELEASES + "/download/SpectrumSetup.exe";
export const LICENSE_URL = REPO + "/blob/main/LICENSE";

/** Measured values, see the measurement report. */
