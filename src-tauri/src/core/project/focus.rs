use regex::Regex;

pub fn validate_project_focus_view_html(html: &str) -> Result<(), String> {
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return Err("Project focus HTML cannot be empty".to_string());
    }

    let lower = trimmed.to_ascii_lowercase();
    let forbidden = [
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

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta
      http-equiv="Content-Security-Policy"
      content="default-src 'none'; script-src 'unsafe-inline' https://cdn.jsdelivr.net; style-src 'unsafe-inline'; img-src data: https:; font-src data: https:; connect-src https://cdn.jsdelivr.net;"
    />
    <title>{title}</title>
    <style>
      :root {{
        --bg: #151814;
        --panel: rgba(28, 33, 29, 0.94);
        --panel-soft: rgba(38, 44, 39, 0.92);
        --line: rgba(224, 214, 194, 0.12);
        --text: #efe7d5;
        --muted: #b9ad98;
        --accent: #dfab52;
        --sage: #9ab488;
        --sky: #86b4d9;
        --danger: #c98274;
      }}
      * {{ box-sizing: border-box; }}
      body {{
        margin: 0;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", serif;
        background:
          radial-gradient(circle at top right, rgba(223,171,82,0.12), transparent 26%),
          radial-gradient(circle at bottom left, rgba(154,180,136,0.10), transparent 22%),
          linear-gradient(180deg, #1b1f1b 0%, var(--bg) 100%);
        color: var(--text);
      }}
      main {{ max-width: 1180px; margin: 0 auto; padding: 24px 22px 34px; }}
      .hero {{
        display: grid;
        grid-template-columns: minmax(0, 1.2fr) minmax(280px, 0.8fr);
        gap: 14px;
        margin-bottom: 14px;
      }}
      .card {{
        border: 1px solid var(--line);
        border-radius: 16px;
        background: linear-gradient(180deg, rgba(255,255,255,0.02), rgba(255,255,255,0.01));
        padding: 18px;
        box-shadow: 0 18px 32px rgba(0,0,0,0.20);
      }}
      .eyebrow {{
        color: var(--accent);
        font-size: 12px;
        letter-spacing: 0.16em;
        text-transform: uppercase;
        margin-bottom: 10px;
      }}
      h1 {{ margin: 0 0 8px; font-size: clamp(30px, 5vw, 52px); line-height: 0.95; }}
      h2 {{ margin: 0 0 10px; font-size: 17px; }}
      p {{ margin: 0; color: var(--muted); line-height: 1.5; }}
      .meta {{ display: flex; gap: 8px; flex-wrap: wrap; margin-top: 16px; }}
      .pill {{
        padding: 6px 10px;
        border-radius: 999px;
        border: 1px solid var(--line);
        background: var(--panel-soft);
        color: var(--muted);
        font-size: 12px;
      }}
      .grid {{
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 14px;
      }}
      .section-label {{
        margin: 0 0 10px;
        color: var(--muted);
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.14em;
      }}
      ul {{
        margin: 0;
        padding-left: 18px;
        color: var(--muted);
      }}
      li + li {{ margin-top: 7px; }}
      .callout {{
        border-left: 3px solid var(--accent);
        padding-left: 12px;
        color: var(--text);
      }}
      .code {{
        font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
        font-size: 12px;
        color: var(--sky);
      }}
      .mermaid {{
        min-height: 240px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 14px;
        border: 1px dashed var(--line);
        background: rgba(0,0,0,0.10);
        padding: 12px;
        overflow: auto;
      }}
      @media (max-width: 900px) {{
        .hero, .grid {{ grid-template-columns: 1fr; }}
        main {{ padding: 18px 14px 24px; }}
      }}
    </style>
  </head>
  <body>
    <!--
      This artifact is for helping the user see the current project situation quickly.
      Do not waste space repeating obvious shell chrome like the project name or generic labels.
      Prefer compact synthesis, comparisons, diagrams, and next-meaningful-state framing.
    -->
    <main>
      <section class="hero">
        <article class="card">
          <div class="eyebrow">Project Focus View</div>
          <h1>Current Picture</h1>
          <p class="callout">
            Keep this page calm, legible, and current. Show what matters now, not raw execution
            chatter.
          </p>
          <div class="meta">
            <span class="pill">Primary surface</span>
            <span class="pill">Machinery on demand</span>
            <span class="pill">Mermaid ready</span>
          </div>
        </article>
        <article class="card">
          <div class="eyebrow">Editing Rules</div>
          <ul>
            <li>Keep the overall structure stable unless the project meaning truly changed.</li>
            <li>Do not restate obvious shell context like the project title or route picker.</li>
            <li>Prefer short bullets, compact status language, and diagrams over long prose.</li>
            <li>Use Mermaid blocks for flows, route comparisons, and architecture snapshots.</li>
            <li>Leave low-level worker activity to machinery, not this page.</li>
          </ul>
        </article>
      </section>

      <section class="grid">
        <article class="card">
          <div class="section-label">Now</div>
          <h2>Current Pursuit</h2>
          <ul>
            <li>Replace this with the active project goal in one sentence.</li>
            <li>Call out the selected route if route choice matters right now.</li>
            <li>Note the nearest decision, proof, or delivery milestone.</li>
          </ul>
        </article>

        <article class="card">
          <div class="section-label">Routes</div>
          <h2>Active Route Picture</h2>
          <ul>
            <li><span class="code">main</span> is the default route.</li>
            <li>Add other active routes only when they materially differ.</li>
            <li>Archive routes instead of leaving stale comparisons here.</li>
          </ul>
        </article>

        <article class="card">
          <div class="section-label">Meaning</div>
          <h2>Decisions And Constraints</h2>
          <ul>
            <li>Document stable choices that should survive route churn.</li>
            <li>Keep constraints crisp: interfaces, boundaries, deployment assumptions.</li>
            <li>If a choice is provisional, move it to questions instead.</li>
          </ul>
        </article>

        <article class="card">
          <div class="section-label">Risk</div>
          <h2>Open Questions</h2>
          <ul>
            <li>What still needs user input, proof, or comparison?</li>
            <li>Which route or experiment is intended to answer it?</li>
            <li>What would change the plan materially?</li>
          </ul>
        </article>

        <article class="card">
          <div class="section-label">Proof</div>
          <h2>Validation</h2>
          <ul>
            <li>State what “done” means for the current push.</li>
            <li>Prefer observable checks: build, behavior, contract, delivery readiness.</li>
            <li>Keep this aligned with the actual execution path.</li>
          </ul>
        </article>

        <article class="card">
          <div class="section-label">Map</div>
          <h2>Project Shape</h2>
          <div class="mermaid">
flowchart TD
  U[User] --> F[Front Desk]
  F --> O[Orchestrators]
  O --> W[Workers]
  O --> R[Routes]
  W --> M[Route Memory]
  O --> D[Project Docs]
  F --> P[Project Focus View]
          </div>
        </article>
      </section>
    </main>
    <script type="module">
      import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';

      mermaid.initialize({{
        startOnLoad: false,
        securityLevel: 'strict',
        theme: 'base',
        fontFamily: 'Iowan Old Style, Palatino Linotype, Book Antiqua, serif',
        themeVariables: {{
          primaryColor: '#222822',
          primaryTextColor: '#efe7d5',
          primaryBorderColor: '#4c5b4f',
          lineColor: '#8ea189',
          secondaryColor: '#2a302b',
          tertiaryColor: '#1c211d',
          clusterBkg: '#1d231e',
          clusterBorder: '#435248',
          background: '#151814',
          mainBkg: '#1f2520',
          nodeTextColor: '#efe7d5'
        }}
      }});

      await mermaid.run({{ querySelector: '.mermaid' }});
    </script>
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
