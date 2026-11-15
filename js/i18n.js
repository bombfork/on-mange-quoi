import { interpolate, lookup, pickLanguage } from "./i18n-core.js";

export const LANGUAGES = {
  fr: { name: "Français" },
};

const DEFAULT_LANGUAGE = "fr";
const STORAGE_KEY = "onmangequoi_language";

let currentLanguage = DEFAULT_LANGUAGE;
let translations = {};

async function loadTranslations(lang) {
  const response = await fetch(`locales/${lang}.json`);
  if (!response.ok) {
    throw new Error(`Failed to load locale "${lang}": HTTP ${response.status}`);
  }
  return response.json();
}

/**
 * Translate a key, replacing `{name}` placeholders from params.
 * Returns the key itself when the translation is missing, so gaps stay visible in the UI.
 * @param {string} keyPath
 * @param {Record<string, unknown>} [params]
 * @returns {string}
 */
export function t(keyPath, params = {}) {
  const value = lookup(translations, keyPath);
  if (typeof value !== "string") {
    console.warn(`Missing translation: ${keyPath}`);
    return keyPath;
  }
  return interpolate(value, params);
}

export function getCurrentLanguage() {
  return currentLanguage;
}

/**
 * Fill the text of every element carrying a `data-i18n="key.path"` attribute.
 * @param {ParentNode} [root]
 */
export function applyTranslations(root = document) {
  for (const element of root.querySelectorAll("[data-i18n]")) {
    element.textContent = t(element.dataset.i18n);
  }
}

/**
 * @param {string} lang - a key of LANGUAGES
 */
export async function setLanguage(lang) {
  if (!Object.hasOwn(LANGUAGES, lang)) {
    throw new Error(`Unsupported language: ${lang}`);
  }
  translations = await loadTranslations(lang);
  currentLanguage = lang;
  localStorage.setItem(STORAGE_KEY, lang);
  document.documentElement.lang = lang;
  applyTranslations();
  window.dispatchEvent(new CustomEvent("languagechange", { detail: { lang } }));
}

/**
 * Language priority: `?lang=` URL parameter, then the stored choice, then browser preferences.
 */
export async function initI18n() {
  const lang = pickLanguage(
    [
      new URLSearchParams(window.location.search).get("lang"),
      localStorage.getItem(STORAGE_KEY),
      ...(navigator.languages ?? [navigator.language]),
    ],
    Object.keys(LANGUAGES),
    DEFAULT_LANGUAGE,
  );
  await setLanguage(lang);
}
