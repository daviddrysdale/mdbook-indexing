# First

{{hi:hidden index entry}}We could always {{i:Rewrite it in Rust}}.

This paragraph includes both {{i:*italics*}} delimited by {{i:`*`}} and {{i:_emphasis_}} delimited by {{i:`_`}}.

This is all to test {{i:[`mdbook-indexing`](https://github.com/daviddrysdale/mdbook-indexing)}}.

Index entries can include {{i:`code`}}.

Sorting for the index should put {{i:`borrow`}} (lowercase) after {{i:`Borrow`}} (uppercase), but preserve case and code
font for both.

Nested index entries might talk about the {{i:PAIR}} protocol or the {{i:PUSH}} protocol. {{hi:protocol}}

Different sorts of {{i:test}}:

- {{hi:test, unit}}unit test
- {{hi:test, integration}}integration test
- {{hi:test, fuzz}} fuzz test
- {{hi: test, doc}} doc test

To use the markup &lbrace;{ii:as is}}, put in an HTML entity reference instead of the markup (e.g. `&lbrace;` instead
of `{`).
