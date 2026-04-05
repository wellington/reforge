.PHONY: e2e verify-access teardown run-reforge validate build

e2e: verify-access teardown run-reforge validate

verify-access:
	@qa/scripts/verify-access.sh

teardown:
	@qa/scripts/teardown.sh

build:
	cargo build --release

run-reforge: build
	@qa/scripts/run-reforge.sh

validate:
	@qa/scripts/validate.sh
