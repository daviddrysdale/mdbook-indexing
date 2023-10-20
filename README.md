# mdbook-indexing

A preprocessor for [mdbook](https://github.com/rust-lang/mdBook) to support building an index.

- Phrases enclosed in `{{i:<text>}}` are transmitted as-is to the rendered output, but also get an index entry added for
  them.
- Phrases enclosed in `{{ii:<text>}}` are transmitted to the rendered output as italics, but also get an index entry
  added for them.
- Phrases enclosed in `{{hi:<text>}}` are removed from the rendered output, but get an index entry added for them
  anyway.
- The contents of any chapter with name **Index** are replaced by the accumulated contents of the index.
   - Note that it's best not to use `index.md` as the filename for the index, as that will become `index.html` and
     end up being the default landing page for the book.  An alternative name (e.g. `indexing.md`) for the file avoids
     this.

## Installation

To use, install the tool

```sh
cargo install mdbook-indexing
```

and add it as a preprocessor in `book.toml`:

```toml
[preprocessor.indexing]
```

## Configuration

### See Instead

Key-value pairs in the `[preprocessor.indexing.see_instead]` section of the `book.toml` configuration file indicate index
entries where the key should point to the value.  Thus an entry like:

```toml
"unit type" = "`()`"
```

would result in an index entry that says: "unit type, see `()`" (instead of a list of locations).

### Nested Entries

Key-value pairs in the `[preprocessor.indexing.nest_under]` section of the `book.toml` configuration file indicate index
entries where the entry for the key should be nested under value.  Thus an entry like:

```toml
"generic type" = "generics"
```

would result in the index entry for "generic type" being only listed as an indented sub-entry under "generics".

### Chapter Names

The `use_chapter_names` boolean config option enables a mode where the generated index uses the names of chapters where
index entries are located, rather than just numbers.

## Limitations

- Avoid putting the index inside a link, as it breaks the link, i.e. prefer:
    ```md
    {{i:[text](http:link)}}
    ```
  to:
    ```md
    [{{i:text}}](http:link)
    ```
