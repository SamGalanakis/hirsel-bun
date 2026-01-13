/**
 * hirsel templates - Manage spec templates
 *
 * Lists available templates for creating new runs.
 */

import { isJsonOutput, jsonOutput, ICONS } from "../index";
import { dim, bold } from "../../shared/theme";
import { existsSync, readdirSync, readFileSync } from "fs";
import { join, dirname } from "path";

// Get templates directory (relative to package)
function getTemplatesDir(): string {
  // Templates are bundled with the package
  const packageDir = dirname(dirname(dirname(import.meta.dir)));
  return join(packageDir, "templates");
}

// List available templates
interface TemplateInfo {
  name: string;
  path: string;
  description: string;
  hasEval: boolean;
}

function listTemplates(): TemplateInfo[] {
  const templatesDir = getTemplatesDir();
  if (!existsSync(templatesDir)) {
    return [];
  }

  const templates: TemplateInfo[] = [];

  try {
    const dirs = readdirSync(templatesDir, { withFileTypes: true })
      .filter(d => d.isDirectory())
      .map(d => d.name);

    for (const name of dirs) {
      const templateDir = join(templatesDir, name);
      const specFile = join(templateDir, "spec.md");

      if (existsSync(specFile)) {
        const content = readFileSync(specFile, "utf-8");
        const firstLine = content.split("\n")[0].trim();
        const description = firstLine.replace(/^#+\s*/, "");

        templates.push({
          name,
          path: templateDir,
          description,
          hasEval: existsSync(join(templateDir, "eval.md")),
        });
      }
    }
  } catch {
    // Return empty list if templates can't be read
  }

  return templates;
}

// Main command handler
export default async function templates(args: string[]): Promise<void> {
  const templateList = listTemplates();

  if (templateList.length === 0) {
    if (isJsonOutput()) {
      jsonOutput({ templates: [] });
    } else {
      console.log(dim("No templates found"));
      console.log();
      console.log(dim("Templates should be in the 'templates/' directory"));
    }
    return;
  }

  if (isJsonOutput()) {
    jsonOutput({ templates: templateList });
    return;
  }

  console.log(bold("\nAvailable Templates\n"));

  for (const t of templateList) {
    const evalMarker = t.hasEval ? dim(" (+eval)") : "";
    console.log(`  ${bold(t.name)}${evalMarker}`);
    console.log(`    ${dim(t.description)}`);
    console.log(`    ${dim(`path: ${t.path}`)}`);
    console.log();
  }

  console.log(dim("Usage: hirsel go <run> --template <name>"));
}
