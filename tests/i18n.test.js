import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { interpolate, lookup, pickLanguage } from "../js/i18n-core.js";

describe("lookup", () => {
  const tree = { app: { title: "On mange quoi ?" }, empty: null };

  it("resolves nested keys", () => {
    assert.equal(lookup(tree, "app.title"), "On mange quoi ?");
  });

  it("returns undefined for missing segments", () => {
    assert.equal(lookup(tree, "app.missing"), undefined);
    assert.equal(lookup(tree, "nope.title"), undefined);
    assert.equal(lookup(tree, "empty.title"), undefined);
  });

  it("ignores inherited properties", () => {
    assert.equal(lookup(tree, "app.toString"), undefined);
  });
});

describe("interpolate", () => {
  it("replaces every occurrence of each placeholder", () => {
    assert.equal(interpolate("{n} repas, {n} recettes", { n: 3 }), "3 repas, 3 recettes");
  });

  it("leaves unknown placeholders untouched", () => {
    assert.equal(interpolate("{a} et {b}", { a: "x" }), "x et {b}");
  });
});

describe("pickLanguage", () => {
  const supported = ["fr"];

  it("returns the first supported candidate, matching the base subtag", () => {
    assert.equal(pickLanguage([null, "en-US", "fr-CA"], supported, "fr"), "fr");
  });

  it("falls back when nothing matches", () => {
    assert.equal(pickLanguage(["en", undefined, ""], ["fr", "es"], "es"), "es");
  });
});
