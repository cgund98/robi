.DEFAULT_GOAL := help

CARGO := cargo
API_BIN := robi-api
OPENAPI_SPEC := openapi/openapi.json

.PHONY: help
help:
	@echo "Common commands:"
	@echo "  make api           - run the local API"
	@echo "  make dev-api       - run the local API and restart it on Rust changes"
	@echo "  make lint          - check Rust and frontend formatting and lints"
	@echo "  make fix           - format and apply lint fixes"
	@echo "  make test          - run the Rust workspace tests"
	@echo "  make openapi-spec  - write $(OPENAPI_SPEC)"
	@echo "  make docs          - build the mdBook documentation"
	@echo "  make docs-serve    - serve the docs locally and open a browser"

.PHONY: api
api:
	$(CARGO) run -p robi --bin $(API_BIN)

.PHONY: dev-api
dev-api:
	@command -v cargo-watch >/dev/null 2>&1 || ( \
		echo "cargo-watch is not installed. Install it with:"; \
		echo "  cargo install cargo-watch --locked"; \
		exit 1 \
	)
	cargo watch -w Cargo.toml -w Cargo.lock -w crates/robi -w crates/robi-core -x 'run -p robi --bin $(API_BIN)'

.PHONY: lint
lint:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	pnpm run lint
	scripts/check-doc-links.sh

.PHONY: fix
fix:
	$(CARGO) clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D warnings
	$(CARGO) fmt --all
	pnpm run lint:fix

.PHONY: test
test:
	$(CARGO) test --workspace

.PHONY: openapi-spec
openapi-spec:
	mkdir -p $(dir $(OPENAPI_SPEC))
	$(CARGO) run -p robi --bin export-openapi > $(OPENAPI_SPEC)

.PHONY: docs
docs:
	mdbook build docs

.PHONY: docs-serve
docs-serve:
	mdbook serve docs --open
