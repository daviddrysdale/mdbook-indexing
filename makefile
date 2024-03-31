all: build
check: build clippy test compare
build:
	cargo build
clippy:
	cargo clippy
test:
	cargo test
# Note that new versions of mdBook will change the surrounding HTML and likely
# require an update to the expected index.
compare: testbook/book/indexing.html
	diff testbook/book/indexing.html testbook/expected_index.html
testbook/book/indexing.html:
	(cd testbook && mdbook build)
