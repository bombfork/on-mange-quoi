#!/usr/bin/env node
// Validates every locales/*.json file against locales/schema.json.

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import Ajv from "ajv";

const localesDir = join(import.meta.dirname, "../locales");
const schema = JSON.parse(readFileSync(join(localesDir, "schema.json"), "utf8"));
const validate = new Ajv({ allErrors: true }).compile(schema);

const localeFiles = readdirSync(localesDir)
  .filter((file) => file.endsWith(".json") && file !== "schema.json")
  .sort();

let failed = false;

for (const file of localeFiles) {
  try {
    const data = JSON.parse(readFileSync(join(localesDir, file), "utf8"));
    if (validate(data)) {
      console.log(`✓ ${file}`);
      continue;
    }
    failed = true;
    console.log(`✗ ${file}`);
    for (const error of validate.errors ?? []) {
      console.log(
        `  ${error.instancePath || "/"}: ${error.message} ${JSON.stringify(error.params)}`,
      );
    }
  } catch (error) {
    failed = true;
    console.log(`✗ ${file}: ${error.message}`);
  }
}

if (localeFiles.length === 0) {
  console.log("No locale files found.");
  failed = true;
}

process.exit(failed ? 1 : 0);
