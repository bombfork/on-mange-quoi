/**
 * Pure i18n helpers, free of DOM and browser globals so they run under `node --test`.
 */

/**
 * Resolve a dot-separated key path in a nested translations object.
 * @param {object} tree
 * @param {string} keyPath - e.g. "landing.intro"
 * @returns {unknown} the value, or undefined when any segment is missing
 */
export function lookup(tree, keyPath) {
  let node = tree;
  for (const key of keyPath.split(".")) {
    if (node === null || typeof node !== "object" || !Object.hasOwn(node, key)) {
      return undefined;
    }
    node = node[key];
  }
  return node;
}

/**
 * Replace every `{name}` placeholder with `params.name`. Unknown placeholders are left as is.
 * @param {string} template
 * @param {Record<string, unknown>} params
 * @returns {string}
 */
export function interpolate(template, params = {}) {
  return template.replace(/\{(\w+)\}/g, (match, name) =>
    Object.hasOwn(params, name) ? String(params[name]) : match,
  );
}

/**
 * Pick the first supported language among candidates, matching on the base subtag
 * ("fr-CA" matches "fr"). Empty candidates are skipped.
 * @param {Array<string | null | undefined>} candidates - in priority order
 * @param {string[]} supported
 * @param {string} fallback
 * @returns {string}
 */
export function pickLanguage(candidates, supported, fallback) {
  for (const candidate of candidates) {
    if (!candidate) {
      continue;
    }
    const base = candidate.toLowerCase().split("-")[0];
    if (supported.includes(base)) {
      return base;
    }
  }
  return fallback;
}
