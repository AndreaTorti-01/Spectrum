// Theme toggle (System -> Light -> Dark), persisted in localStorage. The resolved theme lives on <html data-theme>.
import { THEME_EVENT } from "./env.ts";

type Pref = "system" | "light" | "dark";
const ORDER: Pref[] = ["system", "light", "dark"];
const LABEL: Record<Pref, string> = { system: "System", light: "Light", dark: "Dark" };
const KEY = "spectrum-theme";

const root = document.documentElement;
const light = matchMedia("(prefers-color-scheme: light)");

function resolve(pref: Pref): "light" | "dark" {
  return pref === "system" ? (light.matches ? "light" : "dark") : pref;
}

function setLabel(pref: Pref) {
  const el = document.querySelector(".theme-label");
  if (el) el.textContent = LABEL[pref];
}

function apply(pref: Pref, persist: boolean) {
  root.setAttribute("data-theme", resolve(pref));
  root.setAttribute("data-theme-pref", pref);
  setLabel(pref);
  if (persist) {
    try {
      if (pref === "system") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, pref);
    } catch {
      /* storage can be blocked */
    }
  }
  window.dispatchEvent(new Event(THEME_EVENT));
}

export function initNav() {
  const current = (): Pref => {
    const p = root.getAttribute("data-theme-pref");
    return p === "light" || p === "dark" ? p : "system";
  };
  setLabel(current());
  document.querySelector(".theme-toggle")?.addEventListener("click", () => {
    apply(ORDER[(ORDER.indexOf(current()) + 1) % ORDER.length]!, true);
  });
  light.addEventListener("change", () => {
    if (current() === "system") apply("system", false);
  });
}
