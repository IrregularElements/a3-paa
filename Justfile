all: lint build clippy test fuzz

lint:
	cargo +nightly clippy -- \
		-W bad_style -W const_err -W dead_code -W improper_ctypes \
		-W non_shorthand_field_patterns -W no_mangle_generic_items \
		-W overflowing_literals -W path_statements -W noop_method_call \
		-W patterns_in_fns_without_body -W private_in_public \
		-W unconditional_recursion -W unused -W unused_allocation \
		-W unused_comparisons -W unused_parens -W while_true -W unreachable_pub \
		-W missing_docs -W trivial_casts -W trivial_numeric_casts \
		-W unused_extern_crates -W unused_import_braces -W unused_qualifications -W unused_results \
		-W missing_debug_implementations -W missing_copy_implementations \
		-W explicit-outlives-requirements -W keyword-idents -W macro-use-extern-crate \
		-W meta-variable-misuse -W pointer-structural-match -W rust-2021-incompatible-closure-captures \
		-W rust-2021-incompatible-or-patterns -W rust-2021-prefixes-incompatible-syntax \
		-W rust-2021-prelude-collisions -W single-use-lifetimes -W trivial-casts -W trivial-numeric-casts \
		-W unreachable-pub -W unsafe-op-in-unsafe-fn -W unused-crate-dependencies -W unused-extern-crates \
		-W unused-import-braces -W unused-lifetimes -W variant-size-differences -W clippy::integer_arithmetic \
		-W clippy::cast_possible_truncation

build:
	cargo hack --feature-powerset build
	cargo hack --feature-powerset build --release

clippy:
	cargo hack --feature-powerset clippy

test:
	cargo hack --feature-powerset test

fuzz:
	for TARGET in `cargo fuzz list` ; do timeout 10m cargo fuzz run $TARGET || true ; done
