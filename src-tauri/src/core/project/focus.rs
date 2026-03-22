use regex::Regex;

pub fn validate_project_focus_view_html(html: &str) -> Result<(), String> {
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return Err("Project focus HTML cannot be empty".to_string());
    }

    let lower = trimmed.to_ascii_lowercase();
    let forbidden = [
        "<script",
        "javascript:",
        "<iframe",
        "<object",
        "<embed",
        "<form",
        "<link",
        "<meta http-equiv=\"refresh\"",
        "<meta http-equiv='refresh'",
    ];
    if let Some(pattern) = forbidden.iter().find(|pattern| lower.contains(**pattern)) {
        return Err(format!(
            "Project focus HTML contains a forbidden construct: {}",
            pattern
        ));
    }

    let event_handler = Regex::new(r#"on[a-z]+\s*="#).map_err(|e| e.to_string())?;
    if event_handler.is_match(&lower) {
        return Err("Project focus HTML cannot include inline event handlers".to_string());
    }

    let external_ref =
        Regex::new(r#"(src|href)\s*=\s*["']https?://"#).map_err(|e| e.to_string())?;
    if external_ref.is_match(&lower) {
        return Err(
            "Project focus HTML must be self-contained and cannot reference external assets"
                .to_string(),
        );
    }

    Ok(())
}

pub fn default_project_focus_html(project_name: &str) -> String {
    let title = escape_html(project_name);
    let project_name = escape_html(project_name);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{title}</title>
    <style>
      :root {{
        --bg: #181b17;
        --panel: rgba(33, 38, 33, 0.94);
        --panel-soft: rgba(46, 54, 47, 0.92);
        --line: rgba(224, 214, 194, 0.14);
        --text: #efe7d5;
        --muted: #c5b79e;
        --accent: #dfab52;
        --sage: #9ab488;
      }}
      * {{ box-sizing: border-box; }}
      body {{
        margin: 0;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", serif;
        background:
          radial-gradient(circle at top right, rgba(223,171,82,0.12), transparent 28%),
          radial-gradient(circle at bottom left, rgba(154,180,136,0.12), transparent 24%),
          linear-gradient(180deg, #1d221d 0%, var(--bg) 100%);
        color: var(--text);
      }}
      main {{ max-width: 1160px; margin: 0 auto; padding: 30px 26px 42px; }}
      .hero {{
        display: grid;
        grid-template-columns: 1.35fr 0.85fr;
        gap: 18px;
        margin-bottom: 18px;
      }}
      .card {{
        border: 1px solid var(--line);
        border-radius: 18px;
        background: linear-gradient(180deg, rgba(255,255,255,0.02), rgba(255,255,255,0.01));
        padding: 20px;
        box-shadow: 0 18px 32px rgba(0,0,0,0.22);
      }}
      .eyebrow {{
        color: var(--accent);
        font-size: 12px;
        letter-spacing: 0.16em;
        text-transform: uppercase;
        margin-bottom: 12px;
      }}
      h1 {{ margin: 0 0 10px; font-size: clamp(32px, 5vw, 54px); line-height: 0.96; }}
      h2 {{ margin: 0 0 10px; font-size: 18px; }}
      p {{ margin: 0; color: var(--muted); line-height: 1.55; }}
      .meta {{ display: flex; gap: 10px; flex-wrap: wrap; margin-top: 18px; }}
      .pill {{
        padding: 7px 11px;
        border-radius: 999px;
        border: 1px solid var(--line);
        background: var(--panel-soft);
        color: var(--muted);
        font-size: 12px;
      }}
      .grid {{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }}
      .empty {{
        min-height: 98px;
        display: flex;
        align-items: center;
        justify-content: center;
        text-align: center;
        border: 1px dashed var(--line);
        border-radius: 14px;
        background: rgba(0,0,0,0.08);
        padding: 16px;
      }}
      .highlight {{ color: var(--text); }}
      @media (max-width: 900px) {{
        .hero, .grid {{ grid-template-columns: 1fr; }}
        main {{ padding: 20px 16px 28px; }}
      }}
    </style>
  </head>
  <body>
    <main>
      <section class="hero">
        <article class="card">
          <div class="eyebrow">Project Focus View</div>
          <h1>{project_name}</h1>
          <p>
            This is the project-level focus artifact. Shepherd should keep it calm, legible, and
            incrementally maintained as the project meaning changes.
          </p>
          <div class="meta">
            <span class="pill">Project: {project_name}</span>
            <span class="pill">Primary surface: project focus</span>
            <span class="pill">Machinery: on demand</span>
          </div>
        </article>
        <article class="card">
          <div class="eyebrow">How To Use This View</div>
          <p>
            Summarize what the user should care about now: the current pursuit, route comparisons,
            decisions, open questions, validations, and retained understanding. Leave low-level
            execution detail to the machinery reveal.
          </p>
        </article>
      </section>

      <section class="grid">
        <article class="card"><h2>Current Pursuit</h2><div class="empty">Capture the active project goal here.</div></article>
        <article class="card"><h2>Routes</h2><div class="empty">Summarize the active routes and why they differ.</div></article>
        <article class="card"><h2>Decisions</h2><div class="empty">Record stable choices that shape the project.</div></article>
        <article class="card"><h2>Open Questions</h2><div class="empty">Surface what still needs clarification or proof.</div></article>
        <article class="card"><h2>Validation</h2><div class="empty">Describe what will count as done.</div></article>
        <article class="card"><h2>Retained Understanding</h2><div class="empty">Capture durable context that should remain true across route work.</div></article>
      </section>
    </main>
  </body>
</html>"#
    )
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
