import type { Run } from "@/lib/run";

export const camelVarName = (slug: string): string => slug.replace(/-/g, "");

export const serializeRun = (run: Run): string => {
  const body = JSON.stringify(run, null, 2);
  return `import { defineRun } from "@/lib/run";

export const ${camelVarName(run.slug)} = defineRun(${body});
`;
};

export const addRunToIndex = (
  source: string,
  { slug, varName }: { slug: string; varName: string },
): string => {
  if (source.includes(`from "./${slug}"`)) return source;

  const importLine = `import { ${varName} } from "./${slug}";`;
  const lines = source.split("\n");
  const lastImportIdx = lines.reduce(
    (last, line, i) => (line.startsWith("import ") ? i : last),
    0,
  );
  const withImport = [
    ...lines.slice(0, lastImportIdx + 1),
    importLine,
    ...lines.slice(lastImportIdx + 1),
  ].join("\n");

  return withImport.replace(/\[([^\]]*)\]\.sort/, (_m, inner: string) => {
    const trimmed = inner.trim();
    return `[${trimmed.length ? `${trimmed}, ${varName}` : varName}].sort`;
  });
};
