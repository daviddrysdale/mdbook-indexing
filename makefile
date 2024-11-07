all: build
check: build clippy test install compare
build:
	cargo build
clippy:
	cargo clippy
test:
	cargo test
install:
	cargo install --path .
# Note that new versions of mdBook will change the surrounding HTML and likely
# require an update to the expected index.
compare: testbook/book/indexing.html
	diff testbook/book/indexing.html testbook/expected_index.html
regenerate: testbook/book/indexing.html
	cp testbook/book/indexing.html testbook/expected_index.html
testbook/book/indexing.html: install
	(cd testbook && mdbook build)
