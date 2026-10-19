//! Spec 41: shortcut labels go through `Context::format_shortcut`, which prints the command key as the
//! macOS symbol on a Mac and as `Ctrl` elsewhere. A literal that spells the Mac key out is wrong on
//! Windows and Linux, so no string or char literal under `src/ui/` may hold one. Comments may.

use std::path::{Path, PathBuf};

/// The two spellings, built so this file's own literals hold neither.
const CMD_PLUS: &str = concat!("Cm", "d+");
const CMD_SYMBOL: char = match char::from_u32(0x2318) {
    Some(c) => c,
    None => panic!("U+2318 is a char"),
};

fn spells_the_mac_key(value: &str) -> bool {
    value.contains(CMD_PLUS) || value.contains(CMD_SYMBOL)
}

/// Every string and char literal in `source`, as the line it starts on and its value with escapes
/// decoded. Comments are skipped, nested block comments included; lifetimes and labels are not
/// literals.
fn literals(source: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c == '/' && at(i + 1) == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == '*' {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && at(i + 1) == '*' {
                    (depth, i) = (depth + 1, i + 2);
                } else if chars[i] == '*' && at(i + 1) == '/' {
                    (depth, i) = (depth - 1, i + 2);
                    if depth == 0 {
                        break;
                    }
                } else {
                    line += usize::from(chars[i] == '\n');
                    i += 1;
                }
            }
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let hashes = chars[i..].iter().take_while(|&&h| h == '#').count();
            if matches!(word.as_str(), "r" | "br" | "cr") && at(i + hashes) == '"' {
                let (begun, close) = (line, std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect::<Vec<_>>());
                i += hashes + 1;
                let mut value = String::new();
                while i < chars.len() && !chars[i..].starts_with(&close) {
                    line += usize::from(chars[i] == '\n');
                    value.push(chars[i]);
                    i += 1;
                }
                i += close.len();
                out.push((begun, value));
            }
        } else if c == '"' {
            let begun = line;
            let mut value = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i = escape(&chars, i, &mut value, &mut line);
                } else {
                    line += usize::from(chars[i] == '\n');
                    value.push(chars[i]);
                    i += 1;
                }
            }
            i += 1;
            out.push((begun, value));
        } else if c == '\'' && (at(i + 1) == '\\' || at(i + 2) == '\'') {
            let mut value = String::new();
            i += 1;
            if chars[i] == '\\' {
                i = escape(&chars, i, &mut value, &mut line);
            } else {
                value.push(chars[i]);
                i += 1;
            }
            i += 1;
            out.push((line, value));
        } else {
            i += 1;
        }
    }
    out
}

/// Decodes the escape starting at the backslash `chars[i]` into `value`, returning the index after it.
fn escape(chars: &[char], i: usize, value: &mut String, line: &mut usize) -> usize {
    let next = chars.get(i + 1).copied().unwrap_or('\0');
    match next {
        'n' => value.push('\n'),
        't' => value.push('\t'),
        'r' => value.push('\r'),
        '0' => value.push('\0'),
        'u' => {
            let close = chars[i..].iter().position(|&c| c == '}').map_or(chars.len(), |p| i + p);
            let hex: String = chars[i + 3..close].iter().filter(|&&c| c != '_').collect();
            value.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            return close + 1;
        }
        'x' => {
            let hex: String = chars[i + 2..i + 4].iter().collect();
            value.extend(u8::from_str_radix(&hex, 16).ok().map(char::from));
            return i + 4;
        }
        '\n' => {
            // A line continuation: the newline and the next line's leading whitespace are dropped.
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                *line += usize::from(chars[j] == '\n');
                j += 1;
            }
            return j;
        }
        other => value.push(other),
    }
    i + 2
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_lexer_reads_string_and_char_literals_and_skips_comments() {
    let source = format!(
        "// {CMD_PLUS}C in a comment\n/* outer /* {CMD_SYMBOL} */ still a comment */\nfn f<'a>(x: &'a str) {{\n    let s = \"one\\\n        {CMD_PLUS}Z\";\n    let r = r#\"say \"{CMD_PLUS}K\"\"#;\n    let c = '{CMD_SYMBOL}';\n    let u = \"\\u{{2318}}V\";\n    let q = '\\'';\n    let fine = \"Ctrl+C\";\n}}\n"
    );
    let found: Vec<(usize, String)> = literals(&source);
    let values: Vec<&str> = found.iter().map(|(_, v)| v.as_str()).collect();
    assert_eq!(values, [format!("one{CMD_PLUS}Z").as_str(), &format!("say \"{CMD_PLUS}K\""), &CMD_SYMBOL.to_string(), &format!("{CMD_SYMBOL}V"), "'", "Ctrl+C"]);
    let flagged: Vec<usize> = found.iter().filter(|(_, v)| spells_the_mac_key(v)).map(|(n, _)| *n).collect();
    assert_eq!(flagged, [4, 6, 7, 8], "each literal reports the line it starts on; the comments are not read");
}

#[test]
fn no_literal_under_src_ui_spells_out_the_mac_command_key() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src/ui"), &mut files);
    files.sort();
    let offenders: Vec<String> = files
        .iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(path).unwrap();
            let rel = path.strip_prefix(root).unwrap().display().to_string();
            literals(&source).into_iter().filter(|(_, v)| spells_the_mac_key(v)).map(move |(n, v)| format!("{rel}:{n}: {v:?}"))
        })
        .collect();
    assert!(!files.is_empty());
    assert!(
        offenders.is_empty(),
        "build shortcut labels with ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key)) so Windows and Linux show Ctrl:\n{}",
        offenders.join("\n")
    );
}
