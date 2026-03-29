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
        --panel: rgba(24, 28, 25, 0.94);
        --panel-soft: rgba(32, 37, 33, 0.92);
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
      main {{
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 28px 20px;
      }}
      .card {{
        border: 1px solid var(--line);
        border-radius: 24px;
        background:
          linear-gradient(180deg, rgba(255,255,255,0.025), rgba(255,255,255,0.012)),
          rgba(19, 23, 20, 0.82);
        padding: 28px;
        width: min(760px, 100%);
        box-shadow: 0 24px 80px rgba(0,0,0,0.22);
      }}
      .eyebrow {{
        color: var(--accent);
        font-size: 12px;
        letter-spacing: 0.16em;
        text-transform: uppercase;
        margin-bottom: 10px;
      }}
      h1 {{ margin: 0; font-size: clamp(34px, 5vw, 56px); line-height: 0.96; }}
      p {{ margin: 0; color: var(--muted); line-height: 1.6; }}
      .lede {{
        margin-top: 14px;
        max-width: 44rem;
        font-size: 18px;
      }}
      .meta {{ display: flex; gap: 8px; flex-wrap: wrap; margin-top: 20px; }}
      .pill {{
        padding: 6px 10px;
        border-radius: 999px;
        border: 1px solid var(--line);
        background: var(--panel-soft);
        color: var(--muted);
        font-size: 12px;
      }}
      .placeholder {{
        margin-top: 24px;
        border: 1px dashed var(--line);
        border-radius: 18px;
        padding: 18px 18px 16px;
        background: rgba(0, 0, 0, 0.08);
      }}
      ul {{
        margin: 12px 0 0;
        padding-left: 18px;
        color: var(--muted);
      }}
      li + li {{ margin-top: 7px; }}
      @media (max-width: 900px) {{
        main {{ padding: 16px 12px; }}
        .card {{ padding: 22px 18px; border-radius: 20px; }}
        .lede {{ font-size: 16px; }}
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
      <article class="card">
        <div class="eyebrow">Project Focus View</div>
        <h1>Ready when you are.</h1>
        <p class="lede">
          This surface starts intentionally empty. Once it has been edited, it should illustrate
          what matters now with concise status, thread comparisons, diagrams, and decisions.
        </p>
        <div class="meta">
          <span class="pill">Primary surface</span>
          <span class="pill">Mermaid ready</span>
          <span class="pill">No duplicate chrome</span>
        </div>
        <section class="placeholder">
          <p>
            Use this space for synthesis, not raw logs. Show the current picture only after the
            project has real context worth presenting.
          </p>
          <ul>
            <li>Prefer current goal, thread comparisons, decisions, and open questions.</li>
            <li>Skip obvious shell context like the project title or thread count.</li>
            <li>Use Mermaid only when a diagram clarifies something materially.</li>
          </ul>
        </section>
      </article>
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
