pub(crate) const TEXT_PATCH_INSTRUCTIONS: &str = r#"Patch one text field in place using a line-based patch:

*** Begin Patch
@@ optional anchor line
 unchanged context
-old line
+new line
*** End Patch

Rules:
- Use exactly one `*** Begin Patch` / `*** End Patch` envelope.
- Use ` ` context lines, `-` removals, and `+` additions.
- `@@ label` is an anchor to help locate the hunk. `@@` is allowed for an unlabeled chunk.
- Multiple update hunks are allowed.
- This tool edits one existing text field only. It does not create or delete graph nodes."#;

use regex::Regex;

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const EOF_MARKER: &str = "*** End of File";
const CHANGE_CONTEXT_MARKER: &str = "@@ ";
const EMPTY_CHANGE_CONTEXT_MARKER: &str = "@@";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextPatchOutcome {
    pub(crate) new_text: String,
    pub(crate) added_lines: usize,
    pub(crate) removed_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPatch {
    chunks: Vec<UpdateChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateChunk {
    change_context: Option<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    is_end_of_file: bool,
}

pub(crate) fn apply_text_patch(original: &str, input: &str) -> Result<TextPatchOutcome, String> {
    let patch = parse_patch(input)?;
    if patch.chunks.is_empty() {
        return Err("Patch did not contain any update hunks".to_string());
    }

    let original_had_trailing_newline = original.ends_with('\n');
    let mut original_lines: Vec<String> = original.split('\n').map(String::from).collect();
    if original_had_trailing_newline {
        original_lines.pop();
    }

    let replacements = compute_replacements(&original_lines, &patch.chunks)?;
    let new_lines = apply_replacements(original_lines, &replacements);
    let mut new_text = new_lines.join("\n");
    if original_had_trailing_newline {
        new_text.push('\n');
    }

    let added_lines = patch
        .chunks
        .iter()
        .map(|chunk| chunk.new_lines.len())
        .sum::<usize>();
    let removed_lines = patch
        .chunks
        .iter()
        .map(|chunk| chunk.old_lines.len())
        .sum::<usize>();

    Ok(TextPatchOutcome {
        new_text,
        added_lines,
        removed_lines,
    })
}

fn parse_patch(input: &str) -> Result<ParsedPatch, String> {
    let normalized = input.trim();
    let lines: Vec<&str> = normalized.lines().collect();
    let (first, last) = match lines.as_slice() {
        [] => (None, None),
        [first] => (Some(first.trim()), Some(first.trim())),
        [first, .., last] => (Some(first.trim()), Some(last.trim())),
    };

    match (first, last) {
        (Some(BEGIN_PATCH_MARKER), Some(END_PATCH_MARKER)) => {}
        (Some(first), _) if first != BEGIN_PATCH_MARKER => {
            return Err("The first line of the patch must be '*** Begin Patch'".to_string());
        }
        _ => return Err("The last line of the patch must be '*** End Patch'".to_string()),
    }

    if lines.len() <= 2 {
        return Ok(ParsedPatch { chunks: Vec::new() });
    }

    let mut chunks = Vec::new();
    let mut remaining = &lines[1..lines.len() - 1];
    let mut line_number = 2usize;
    while !remaining.is_empty() {
        if remaining[0].trim().is_empty() {
            remaining = &remaining[1..];
            line_number += 1;
            continue;
        }
        let (chunk, consumed) = parse_update_chunk(remaining, line_number, chunks.is_empty())?;
        chunks.push(chunk);
        remaining = &remaining[consumed..];
        line_number += consumed;
    }

    Ok(ParsedPatch { chunks })
}

fn parse_update_chunk(
    lines: &[&str],
    line_number: usize,
    allow_missing_context: bool,
) -> Result<(UpdateChunk, usize), String> {
    if lines.is_empty() {
        return Err(format!(
            "invalid patch at line {line_number}: missing update chunk"
        ));
    }

    let (change_context, start_index) = if lines[0].trim_end() == EMPTY_CHANGE_CONTEXT_MARKER {
        (None, 1)
    } else if let Some(context) = lines[0].strip_prefix(CHANGE_CONTEXT_MARKER) {
        (Some(context.to_string()), 1)
    } else if allow_missing_context {
        (None, 0)
    } else {
        return Err(format!(
            "invalid patch at line {line_number}: expected chunk to start with '@@' or '@@ label'"
        ));
    };

    if start_index >= lines.len() {
        return Err(format!(
            "invalid patch at line {}: update chunk does not contain any lines",
            line_number + 1
        ));
    }

    let mut chunk = UpdateChunk {
        change_context,
        old_lines: Vec::new(),
        new_lines: Vec::new(),
        is_end_of_file: false,
    };
    let mut parsed_lines = 0usize;

    for line in &lines[start_index..] {
        match *line {
            EOF_MARKER => {
                if parsed_lines == 0 {
                    return Err(format!(
                        "invalid patch at line {}: update chunk does not contain any lines",
                        line_number + 1
                    ));
                }
                chunk.is_end_of_file = true;
                parsed_lines += 1;
                break;
            }
            line_contents => match line_contents.chars().next() {
                None => {
                    chunk.old_lines.push(String::new());
                    chunk.new_lines.push(String::new());
                    parsed_lines += 1;
                }
                Some(' ') => {
                    chunk.old_lines.push(line_contents[1..].to_string());
                    chunk.new_lines.push(line_contents[1..].to_string());
                    parsed_lines += 1;
                }
                Some('+') => {
                    chunk.new_lines.push(line_contents[1..].to_string());
                    parsed_lines += 1;
                }
                Some('-') => {
                    chunk.old_lines.push(line_contents[1..].to_string());
                    parsed_lines += 1;
                }
                _ => {
                    if parsed_lines == 0 {
                        return Err(format!(
                            "invalid patch at line {}: unexpected line in update chunk",
                            line_number + 1
                        ));
                    }
                    break;
                }
            },
        }
    }

    Ok((chunk, parsed_lines + start_index))
}

fn compute_replacements(
    original_lines: &[String],
    chunks: &[UpdateChunk],
) -> Result<Vec<(usize, usize, Vec<String>)>, String> {
    let mut replacements = Vec::new();
    let mut line_index = 0usize;

    for chunk in chunks {
        if let Some(ctx_line) = &chunk.change_context {
            if let Some(index) = seek_sequence(
                original_lines,
                std::slice::from_ref(ctx_line),
                line_index,
                false,
            ) {
                line_index = index;
            } else {
                return Err(format!("Failed to find anchor context: {ctx_line}"));
            }
        }

        if chunk.old_lines.is_empty() {
            let insertion_idx = if original_lines.is_empty() {
                0
            } else {
                line_index.min(original_lines.len())
            };
            replacements.push((insertion_idx, 0, chunk.new_lines.clone()));
            continue;
        }

        let mut pattern: &[String] = &chunk.old_lines;
        let mut replacement: &[String] = &chunk.new_lines;
        let mut found = seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);

        if found.is_none() && pattern.last().is_some_and(String::is_empty) {
            pattern = &pattern[..pattern.len() - 1];
            if replacement.last().is_some_and(String::is_empty) {
                replacement = &replacement[..replacement.len() - 1];
            }
            found = seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);
        }

        if let Some(start_idx) = found {
            replacements.push((start_idx, pattern.len(), replacement.to_vec()));
            line_index = start_idx + pattern.len();
        } else {
            return Err(format!(
                "Failed to find expected lines:\n{}",
                chunk.old_lines.join("\n")
            ));
        }
    }

    replacements.sort_by_key(|(start_idx, _, _)| *start_idx);
    Ok(replacements)
}

fn apply_replacements(
    mut lines: Vec<String>,
    replacements: &[(usize, usize, Vec<String>)],
) -> Vec<String> {
    for (start_idx, old_len, new_segment) in replacements.iter().rev() {
        for _ in 0..*old_len {
            if *start_idx < lines.len() {
                lines.remove(*start_idx);
            }
        }
        for (offset, line) in new_segment.iter().enumerate() {
            lines.insert(*start_idx + offset, line.clone());
        }
    }
    lines
}

fn seek_sequence(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start);
    }
    if pattern.len() > lines.len() {
        return None;
    }

    let search_start = if eof && lines.len() >= pattern.len() {
        lines.len() - pattern.len()
    } else {
        start
    };

    for i in search_start..=lines.len().saturating_sub(pattern.len()) {
        if lines[i..i + pattern.len()] == *pattern {
            return Some(i);
        }
    }
    for i in search_start..=lines.len().saturating_sub(pattern.len()) {
        let mut ok = true;
        for (p_idx, pat) in pattern.iter().enumerate() {
            if lines[i + p_idx].trim_end() != pat.trim_end() {
                ok = false;
                break;
            }
        }
        if ok {
            return Some(i);
        }
    }
    for i in search_start..=lines.len().saturating_sub(pattern.len()) {
        let mut ok = true;
        for (p_idx, pat) in pattern.iter().enumerate() {
            if normalize_for_match(&lines[i + p_idx]) != normalize_for_match(pat) {
                ok = false;
                break;
            }
        }
        if ok {
            return Some(i);
        }
    }
    None
}

fn normalize_for_match(text: &str) -> String {
    text.trim()
        .chars()
        .map(|ch| match ch {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => ' ',
            other => other,
        })
        .collect()
}

pub(crate) fn is_safe_patch_field_name(field: &str) -> bool {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$")
        .expect("valid field-name regex")
        .is_match(field)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_single_line() {
        let outcome = apply_text_patch(
            "hello\nworld\n",
            "*** Begin Patch\n@@ hello\n-world\n+hirsel\n*** End Patch",
        )
        .unwrap();
        assert_eq!(outcome.new_text, "hello\nhirsel\n");
        assert_eq!(outcome.added_lines, 1);
        assert_eq!(outcome.removed_lines, 1);
    }

    #[test]
    fn inserts_after_anchor() {
        let outcome = apply_text_patch(
            "one\ntwo\n",
            "*** Begin Patch\n@@ one\n+inserted\n*** End Patch",
        )
        .unwrap();
        assert_eq!(outcome.new_text, "inserted\none\ntwo\n");
    }
}
