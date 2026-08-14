//! `libraryfolders.vdf` — the file in which Steam lists its libraries.
//!
//! Hand-rolled, like the CLI parser in the shell and for the same reason: the Core has no
//! dependencies and keeping it that way is what makes the seam cheap to test (see the
//! comment in `crates/core/Cargo.toml`). The format is small — quoted strings, `{ … }`
//! blocks, `//` comments — and only one thing is wanted out of it.
//!
//! Both shapes Steam has written are accepted, because both are still on people's machines:
//!
//! ```text
//! "libraryfolders"                     "LibraryFolders"
//! {                                    {
//!     "0"                                  "TimeNextStatsReport"  "1700000000"
//!     {                                    "1"    "/mnt/games/SteamLibrary"
//!         "path"   "/mnt/games/..."    }
//!         "apps"  { "8930"  "9876" }
//!     }
//! }
//! ```

use std::path::PathBuf;

/// Every library path the file names, in the order it names them.
///
/// Anything that is not a library path is ignored rather than rejected: this file is written
/// by Steam, not by us, and a version of it we do not fully understand should still yield the
/// libraries we do understand.
pub(crate) fn library_paths(text: &str) -> Vec<PathBuf> {
    let tokens = tokenize(text);
    let mut paths = Vec::new();

    let mut index = 0;
    while index + 1 < tokens.len() {
        let (Token::Text(key), Token::Text(value)) = (&tokens[index], &tokens[index + 1]) else {
            index += 1;
            continue;
        };
        // The modern shape names the key; the old shape numbers it. The "looks like a path"
        // test is what keeps the numbered case from swallowing the `"8930" "9876…"` pairs in
        // an `apps` block, which are numbered too.
        if key.eq_ignore_ascii_case("path") || (is_number(key) && looks_like_a_path(value)) {
            paths.push(PathBuf::from(value));
        }
        index += 2;
    }

    paths
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// An absolute path: `/mnt/games/…` on Linux, `D:\SteamLibrary` on Windows. Both appear in
/// this file on both platforms, since a library may have been recorded on either.
fn looks_like_a_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    match bytes {
        [b'/' | b'\\', ..] => true,
        [drive, b':', ..] => drive.is_ascii_alphabetic(),
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Text(String),
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '"' => {
                let mut value = String::new();
                while let Some(character) = characters.next() {
                    match character {
                        '"' => break,
                        // VDF escapes backslashes, which is how a Windows path survives being
                        // written into this file.
                        '\\' => match characters.next() {
                            Some('n') => value.push('\n'),
                            Some('t') => value.push('\t'),
                            Some(escaped) => value.push(escaped),
                            None => break,
                        },
                        _ => value.push(character),
                    }
                }
                tokens.push(Token::Text(value));
            }
            '/' if characters.peek() == Some(&'/') => {
                for character in characters.by_ref() {
                    if character == '\n' {
                        break;
                    }
                }
            }
            _ if character.is_whitespace() => {}
            // An unquoted word. Steam does not write these, but the format allows them.
            _ => {
                let mut value = String::from(character);
                while let Some(next) = characters.peek() {
                    if next.is_whitespace() || *next == '{' || *next == '}' || *next == '"' {
                        break;
                    }
                    value.push(*next);
                    characters.next();
                }
                tokens.push(Token::Text(value));
            }
        }
    }

    tokens
}
