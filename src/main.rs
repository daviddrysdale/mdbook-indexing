//! mdbook preprocessor that assembles an index
//!
//! Phrases enclosed in `{{i:<text>}}` are transmitted as-is to the rendered output, but also get an index entry added for them.
//!
//! Phrases enclosed in `{{hi:<text>}}` are removed from the rendered output, but get an index entry added for them anyway.
//!
//! A book chapter with title "Index" will have its contents replaced by the accumulated index.
//!
//! Key-value pairs in the `[preprocessor.indexing.see_instead]` section of the `book.toml` configuration file indicate index
//! entries where the key should point to the value.  Thus an entry like:
//!
//! ```toml
//! "unit type" = "`()`"
//! ```
//!
//! would result in an index entry that says: "unit type, see `()`" (instead of a list of locations).
//!
//! Key-value pairs in the `[preprocessor.indexing.nest_under]` section of the `book.toml` configuration file indicate index
//! entries where the entry for the key should be nested under value.  Thus an entry like:
//!
//! ```toml
//! "generic type" = "generics"
//! ```
//!
//! would result in the index entry for "generic type" being only listed as an indented sub-entry under "generics".
//!
//! Tips on usage:
//!
//! - Avoid putting the index inside a link, as it breaks the link, i.e. prefer:
//!     ```md
//!     {{i:[text](http:link)}}
//!     ```
//!   to:
//!     ```md
//!     [{{i:text}}](http:link)
//!     ```
//!

use clap::{Arg, Command};
use mdbook_preprocessor::{
    book::{Book, BookItem},
    errors::Error,
    Preprocessor, PreprocessorContext, MDBOOK_VERSION,
};
use regex::Regex;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io,
    path::PathBuf,
    process,
    sync::LazyLock,
};

const NAME: &str = "indexing";

/// Indentation to use for a nest-under entry, e.g.:
///
///   testing,
///        fuzz testing
///   ^^^^^
const NEST_UNDER_INDENT: &str = "&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;";

/// Indentation to use for use-chapter-names entries, e.g.:
///
///   testing
///        Introduction,
///        Tooling,
///   ^^^^^
const USE_NAMES_INDENT: &str = "&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;";

pub fn make_app() -> Command {
    Command::new("index-preprocessor")
        .about("An mdbook preprocessor which collates an index")
        .subcommand(
            Command::new("supports")
                .arg(Arg::new("renderer").required(true))
                .about("Check whether a renderer is supported by this preprocessor"),
        )
}

fn main() {
    env_logger::init();

    let matches = make_app().get_matches();

    if let Some(sub_args) = matches.subcommand_matches("supports") {
        let renderer = sub_args
            .get_one::<String>("renderer")
            .expect("Required argument");
        let supported = Index::supports_renderer(renderer);

        // Signal whether the renderer is supported by exiting with 1 or 0.
        if supported {
            process::exit(0);
        } else {
            process::exit(1);
        }
    } else {
        let (ctx, book) =
            mdbook_preprocessor::parse_input(io::stdin()).expect("Failed to parse input");
        let preprocessor = Index::new(&ctx);
        let processed_book = preprocessor
            .run(&ctx, book)
            .expect("Failed to process book");
        serde_json::to_writer(io::stdout(), &processed_book).expect("Faild to emit processed book");
    }
}

/// Command for a visible index entry.
const VISIBLE: &str = "i";
/// Command for a hidden index entry.
const HIDDEN: &str = "hi";
/// Command for a visible index entry, italicized.
const ITALIC: &str = "ii";

/// Escape character.
const ESCAPE_CHAR: char = '\\';

/// Regular expression to match indexing commands.
static INDEX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)             # insignificant whitespace mode
              (?s)             # dot matches newline
              \\\{\{[^}]*\}\}  # match escaped link
              |                # or
              \{\{             # opening braces
              (?P<viz>[hi]?i)  # visibility command (i, hi, ii)
              :                # separator
              \s*              # ignore leading whitespace
              (?P<content>.*?) # index entry
              \}\}             # closing braces",
    )
    .unwrap()
});

/// Regular expression to match a Markdown link.
static MD_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\[(?P<text>[^]]+)\]\((?P<link>[^)]+)\)").unwrap());
/// Regular expression for whitespace.
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\s+").unwrap());

/// Location of an index anchor in the source book.
#[derive(Clone, Debug)]
struct Location {
    /// File in source book.
    pub path: Option<PathBuf>,
    /// Chapter name in source book.
    pub name: String,
    /// Anchor identifier.
    pub anchor: String,
}

/// A pre-processor that tracks index entries.
pub struct Index {
    /// Renderers for which no indexing content should be emitted.
    skip_renderer: HashSet<String>,
    /// Index entries that redirect to a different entry.
    see_instead: HashMap<String, String>,
    /// Index entries that should appear in the index as sub-entries underneath the specified top-level entry.
    nest_under: HashMap<String, String>,
    /// Whether to skip a "head, " prefix in sub-entries where the prefix matches the top-level entry.
    suppress_head: bool,
    /// Emit chapter names as the link text in the generated index.
    use_chapter_names: bool,
    /// List of index anchor locations for each (canonicalized) index entry.
    entries: RefCell<HashMap<String, Vec<Location>>>,
}

/// Convert index text to a canonical form suitable for inclusion in the index.
fn canonicalize(s: &str) -> String {
    // Remove any links from the index name.
    let delinked = MD_LINK_RE.replace_all(s, "$text").to_string();

    // Canonicalize whitespace.
    WHITESPACE_RE.replace_all(&delinked, " ").to_string()
}

impl Index {
    /// Create a new preprocessor, based on configuration in `ctx`.
    pub fn new(ctx: &PreprocessorContext) -> Self {
        if ctx.mdbook_version != MDBOOK_VERSION {
            // We should probably use the `semver` crate to check compatibility here...
            eprintln!(
                "Warning: The {NAME} plugin was built against version {MDBOOK_VERSION} of mdbook, \
                 but we're being called from version {}",
                ctx.mdbook_version
            );
        }

        let skip_renderer = if let Ok(Some(toml::Value::String(val))) =
            ctx.config.get("preprocessor.indexing.skip_renderer")
        {
            log::info!("Skipping output for renderers in: {val}");
            val.split(',')
                .map(|s| s.to_string())
                .collect::<HashSet<String>>()
        } else {
            HashSet::new()
        };

        let mut see_instead = HashMap::new();
        if let Ok(Some(toml::Value::Table(table))) =
            ctx.config.get("preprocessor.indexing.see_instead")
        {
            for (key, val) in table {
                if let toml::Value::String(value) = val {
                    log::info!("Index entry '{}' will be 'see {}'", key, value);
                    see_instead.insert(key.to_owned(), value.to_owned());
                }
            }
        }

        let mut nest_under = HashMap::new();
        if let Ok(Some(toml::Value::Table(table))) =
            ctx.config.get("preprocessor.indexing.nest_under")
        {
            for (key, val) in table {
                if let toml::Value::String(value) = val {
                    log::info!("Index entry '{}' will be nested under '{}'", key, value);
                    nest_under.insert(key.to_owned(), value.to_owned());
                }
            }
        }

        let mut use_chapter_names = false;
        if let Ok(Some(toml::Value::Boolean(val))) =
            ctx.config.get("preprocessor.indexing.use_chapter_names")
        {
            use_chapter_names = val;
        }

        let mut suppress_head = false;
        if let Ok(Some(toml::Value::Boolean(val))) =
            ctx.config.get("preprocessor.indexing.suppress_head")
        {
            suppress_head = val;
        }

        Self {
            skip_renderer,
            see_instead,
            nest_under,
            use_chapter_names,
            suppress_head,
            entries: RefCell::new(HashMap::new()),
        }
    }

    /// Process a chapter, emitting index anchors and accumulating the index information.
    fn process_chapter(
        &self,
        renderer: &str,
        path: &Option<PathBuf>,
        name: &str,
        content: &str,
    ) -> String {
        let mut count = 1;
        let mut entries = self.entries.borrow_mut();
        INDEX_RE
            .replace_all(content, |caps: &regex::Captures| {
                if let Some(mat) = caps.get(0) {
                    if mat.as_str().starts_with(ESCAPE_CHAR) {
                        return mat.as_str()[1..].to_owned();
                    }
                }
                // Retrieve the content of the markup.  For a visible index entry, this is
                // rendered in the output.
                let viz = caps.name("viz").unwrap().as_str();
                let content = caps.name("content").unwrap().as_str().to_string();
                // Remove any links from the index name and canonicalize whitespace to get
                // what should appear in the index.
                let mut index_entry = canonicalize(&content);
                log::debug!("found {viz} index entry '{content}' which maps to '{index_entry}'");
                // Accumulate location against see_instead target if present
                if let Some(dest) = self.see_instead.get(&index_entry) {
                    index_entry.clone_from(dest);
                    log::debug!("...or in fact '{index_entry}'");
                }

                let (visible, italic) = match viz {
                    ITALIC => (true, true),
                    VISIBLE => (true, false),
                    HIDDEN => (false, false),
                    other => {
                        eprintln!("Unexpected index type {other}!");
                        (false, false)
                    }
                };

                if self.skip_renderer.contains(renderer) {
                    if visible {
                        if italic {
                            format!("*{content}*")
                        } else {
                            "{content}".to_string()
                        }
                    } else {
                        "".to_string()
                    }
                } else if renderer == "asciidoc" {
                    let nest_under = self.nest_under.get(&index_entry);
                    let mut index_entry = text_to_asciidoc(&index_entry);
                    log::debug!("asciidoc entry '{index_entry}'");
                    if let Some(nest_under) = nest_under {
                        let mut nest_under = text_to_asciidoc(nest_under);
                        asciidoc_protect(&mut nest_under);
                        index_entry = format!("{nest_under},\"{index_entry}\"");
                        log::debug!("nested entry '{index_entry}'");
                    } else {
                        asciidoc_protect(&mut index_entry);
                    }
                    // TODO: figure out how to avoid needing the space after the index marker
                    if visible {
                        if italic {
                            format!("indexterm:[{index_entry}] *{content}*")
                        } else {
                            format!("indexterm:[{index_entry}] {content}")
                        }
                    } else {
                        format!("indexterm:[{index_entry}] ")
                    }
                } else {
                    let anchor = format!("a{:03}", count);
                    let location = Location {
                        path: path.clone(),
                        name: name.to_owned(),
                        anchor: anchor.clone(),
                    };
                    count += 1;

                    let itemlist = entries.entry(index_entry).or_default();
                    log::trace!("Index entry '{content}' found at {location:?}");
                    itemlist.push(location);

                    if visible {
                        if italic {
                            format!("<a name=\"{anchor}\"></a>*{content}*")
                        } else {
                            format!("<a name=\"{anchor}\"></a>{content}")
                        }
                    } else {
                        format!("<a name=\"{anchor}\"></a>")
                    }
                }
            })
            .to_string()
    }

    /// Generate the index page.
    pub fn generate_index(&self, renderer: &str) -> String {
        if self.skip_renderer.contains(renderer) {
            return "".to_string();
        } else if renderer == "asciidoc" {
            // AsciiDoc takes care of generating the index catalog.
            return "[index]\n== Index\n".to_string();
        }
        let mut result = String::new();
        result += "# Index\n\n";

        // Sort entries alphabetically, ignoring case and special characters. Need
        // to sort twice:
        // - once by key as-is, so uppercase entries come before lowercase entries
        // - then by lowercased key, so that the order ignores case.
        // This ensures that entries that are the same except for capitalization
        // (e.g. "Borrow" and "borrow") always sort in a consistent order.
        let mut keys: Vec<String> = self.entries.borrow().keys().cloned().collect();
        let see_also_keys: Vec<String> = self.see_instead.keys().cloned().collect();
        keys.extend_from_slice(&see_also_keys);
        keys.sort();
        keys.sort_by_key(|s| {
            s.to_lowercase()
                .chars()
                .filter(|c| !matches!(c, '_' | '*' | '{' | '}' | '`' | '[' | ']' | '@' | '\''))
                .collect::<String>()
        });

        // Remove any sub-entries from the list of keys, and track them separately
        // according to the main entry they will go underneath.
        let mut sub_entries = HashMap::<String, Vec<String>>::new();
        keys.retain(|s| {
            if let Some(head) = self.nest_under.get(s) {
                // This is a sub-entry, so filter it out but also remember it in the per-main
                // entry list.  Because the keys are already sorted, the per-main entry list
                // will also be correctly sorted.
                let entries = sub_entries.entry(head.to_string()).or_default();
                entries.push(s.clone());
                false
            } else {
                true
            }
        });

        for entry in keys {
            result = self.append_entry(result, "", &entry, &entry);

            if let Some(subs) = sub_entries.get(&entry) {
                for sub in subs.iter() {
                    result = self.append_entry(
                        result,
                        NEST_UNDER_INDENT,
                        sub,
                        self.subentry(&entry, sub),
                    );
                }
            }
        }
        result
    }

    /// Generate the display form of a sub-entry.
    fn subentry<'a>(&self, entry: &'_ str, sub: &'a str) -> &'a str {
        if self.suppress_head {
            // See if the sub-entry starts with "{entry}, ".
            if let Some(rest) = sub.strip_prefix(entry) {
                if let Some(inner_sub) = rest.strip_prefix(", ") {
                    return inner_sub;
                }
            }
        }
        sub
    }

    /// Append an entry to the generated index.
    fn append_entry(
        &self,
        mut result: String,
        indent: &str,
        entry: &str,
        entry_display: &str,
    ) -> String {
        result += indent;
        if let Some(alt) = self.see_instead.get(entry) {
            result += &format!("{}, see {}", entry_display, alt);
            // Check that the destination exists.
            if self.entries.borrow().get(alt).is_none() {
                log::error!(
                    "Destination of see_instead '{}' => '{}' not in index!",
                    entry,
                    alt
                );
            }
        } else {
            let locations = self.entries.borrow().get(entry).unwrap().to_vec();
            result += entry_display;
            for (idx, loc) in locations.into_iter().enumerate() {
                let (separator, anchor_text) = if self.use_chapter_names {
                    (
                        format!(",<br/>\n{indent}{USE_NAMES_INDENT}"),
                        loc.name.to_string(),
                    )
                } else {
                    (", ".to_string(), format!("{}", idx + 1))
                };
                result += &separator;
                if let Some(path) = &loc.path {
                    result += &format!(
                        "[{}]({}#{})",
                        anchor_text,
                        path.as_path().display(),
                        loc.anchor
                    );
                } else {
                    result += &anchor_text;
                }
            }
        }
        result += "<br/>\n";
        result
    }

    /// Indicate whether a renderer is supported.
    fn supports_renderer(renderer: &str) -> bool {
        renderer != "not-supported"
    }
}

impl Preprocessor for Index {
    fn name(&self) -> &str {
        NAME
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book, Error> {
        book.for_each_mut(|item| {
            if let BookItem::Chapter(chap) = item {
                if chap.name == "Index" {
                    log::debug!("Replacing chapter named '{}' with contents", chap.name);
                    chap.content = self.generate_index(&ctx.renderer);
                } else {
                    log::info!("Indexing chapter '{}'", chap.name);
                    chap.content =
                        self.process_chapter(&ctx.renderer, &chap.path, &chap.name, &chap.content);
                }
            }
        });
        Ok(book)
    }

    fn supports_renderer(&self, renderer: &str) -> Result<bool, Error> {
        Ok(Self::supports_renderer(renderer))
    }
}

/// Convert index text into a form suitable for AsciiDoc.
fn text_to_asciidoc(text: &str) -> String {
    // Remove surrounding MarkDown formatting characters and substitute for special characters.
    text.replace('`', "")
        .trim_matches('*')
        .trim_matches('_')
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('&', "&amp;")
}

/// Protect a string from AsciiDoc intepretation
/// - Add quotes round a string if it contains commas.
/// - Use a passthrough macro if it contains character replacement substitutions.
fn asciidoc_protect(text: &mut String) {
    if text.contains(',') {
        // An index entry with a comma needs double quotes around it so
        // the comma doesn't induce a nested entry.
        let quoted_text = format!("\"{text}\"");
        *text = quoted_text;
    }
    if text.contains("(C)") {
        // Avoid (C) being interpreted as a copyright sign. (Source can always use &#169; to get one anyway.)
        let pass_text = format!("pass:[{text}]");
        *text = pass_text;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonicalize() {
        use super::canonicalize;
        let cases = vec![
            ("abc", "abc"),
            ("ab cd", "ab cd"),
            ("ab    cd", "ab cd"),
            ("ab    cd", "ab cd"),
            ("ab  	cd", "ab cd"),
            ("ab  \ncd", "ab cd"),
            ("`ab`", "`ab`"),
            ("[`ab`](somedest)", "`ab`"),
            ("[`ab`]", "[`ab`]"),
            ("[`ab    cd`](somedest)", "`ab cd`"),
        ];
        for (input, want) in cases {
            let got = canonicalize(input);
            assert_eq!(got, want, "Mismatch for input: {}", input);
        }
    }

    #[test]
    fn test_matches() {
        let tests = [
            ("blah {{i:simple}} blah", VISIBLE, "simple"),
            ("blah{{i:simple}}blah", VISIBLE, "simple"),
            ("blah {{hi:simple}} blah", HIDDEN, "simple"),
            ("blah {{ii:simple}} blah", ITALIC, "simple"),
            (
                "blah {{i:[link](http://example.com)}} blah",
                VISIBLE,
                "[link](http://example.com)",
            ),
            ("blah {{i:*italic*}} blah", VISIBLE, "*italic*"),
            ("blah {{i:_italic_}} blah", VISIBLE, "_italic_"),
            ("blah {{i:`code`}} blah", VISIBLE, "`code`"),
            ("blah {{i:first}} blah {{hi:second}}", VISIBLE, "first"),
            ("blah {{i:interior space}} blah", VISIBLE, "interior space"),
            (
                "blah {{i:interior\nnewline}} blah",
                VISIBLE,
                "interior\nnewline",
            ),
            (
                "blah {{i:interior\tspace}} blah",
                VISIBLE,
                "interior\tspace",
            ),
            ("blah {{i: leading space}} blah", VISIBLE, "leading space"),
            (
                "blah {{i:trailing space }} blah",
                VISIBLE,
                "trailing space ",
            ),
            ("blah {{i:normal}} blah \\{{i:escaped}}", VISIBLE, "normal"),
        ];
        for (input, want_viz, want_content) in tests {
            let got = INDEX_RE.captures_iter(input).next().unwrap();
            let got_viz = got.name("viz").unwrap().as_str();
            assert_eq!(got_viz, want_viz, "for input '{input}'");
            let got_content = got.name("content").unwrap().as_str();
            assert_eq!(got_content, want_content, "for input '{input}'");
        }
    }

    #[test]
    fn test_escaped_matches() {
        let tests = [
            "blah \\{{i:simple}} blah",
            "blah\\{{i:simple}}blah",
            "blah \\{{hi:simple}} blah",
            "blah \\{{ii:simple}} blah",
            "blah \\{{i:`code`}} blah",
            "blah \\{{i:interior space}} blah",
            "blah \\{{i:interior\nnewline}} blah",
            "blah \\{{i: leading space}} blah",
            "blah \\{{i:trailing space }} blah",
        ];
        for input in tests {
            let got = INDEX_RE.captures_iter(input).next().unwrap();
            assert!(
                got.get(0).unwrap().as_str().starts_with(ESCAPE_CHAR),
                "got {:?} for input '{}'",
                got,
                input
            );
            assert!(got.name("viz").is_none(), "for input '{}'", input);
            assert!(got.name("content").is_none(), "for input '{}'", input);
        }
    }

    #[test]
    fn test_escaped_and_unescaped() {
        let input = "blah \\{{i:escaped}} blah {{i:second}}";
        let mut iter = INDEX_RE.captures_iter(input);
        let got1 = iter.next().unwrap();
        assert!(got1.get(0).unwrap().as_str().starts_with(ESCAPE_CHAR),);
        let got2 = iter.next().unwrap();
        let got2_viz = got2.name("viz").unwrap().as_str();
        assert_eq!(got2_viz, VISIBLE);
        let got2_content = got2.name("content").unwrap().as_str();
        assert_eq!(got2_content, "second");
    }
}
