import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

test("compact shortcut actions stay aligned beside their copy", () => {
  const consoleStyles = readFileSync("src/styles.css", "utf8");

  expect(consoleStyles).toMatch(
    /@media \(max-width: 760px\)[\s\S]*?\.shortcut-setting\s*\{\s*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto/
  );
});

test("update frequency labels never wrap vertically", () => {
  const consoleStyles = readFileSync("src/styles.css", "utf8");

  expect(consoleStyles).toMatch(
    /\.settings-policy-options button\s*\{[\s\S]*?white-space:\s*nowrap/
  );
});

test("version strategy uses stable rows and a separate footer", () => {
  const consoleStyles = readFileSync("src/styles.css", "utf8");

  expect(consoleStyles).toMatch(
    /\.strategy-row\s*\{[\s\S]*?grid-template-columns:\s*minmax\(180px,\s*1fr\)\s+minmax\(300px,\s*360px\)/
  );
  expect(consoleStyles).toMatch(/\.strategy-footer\s*\{/);
});
