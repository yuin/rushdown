RM = rm -f
CMARK_BIN=benches/cmark_benchmark

test: test-nostd

.PHONY: test-nostd
test-nostd: scanner ## Run tests for no-std
	RUST_BACKTRACE=1 cargo test --no-default-features --features no-std-unix-debug,html-entities -- --nocapture --test-threads=1

.PHONY: test-std
test-std: scanner ## Run tests for std
	yes | RUST_BACKTRACE=1 cargo llvm-cov --branch --ignore-filename-regex='test.rs|gen.rs' -- --nocapture --test-threads=1

.PHONY: fuzz
fuzz: scanner ## Run fuzz tests
	cargo fuzz run markdown

scanner: src/scanner/scanner_gen.rs ## Generate the scanner source code

src/scanner/scanner_gen.rs: src/scanner/scanner_record.re src/scanner/scanner_generic.re
	re2rust -W -Werror -i --no-generation-date  src/scanner/scanner_record.re > $@
	re2rust -W -Werror -i --no-generation-date src/scanner/scanner_generic.re >> $@
	cargo fmt

.PHONY: profile
profile: ## Profiles the release build
	CARGO_RELEASE_DEBUG=true cargo build --release --features profile --bin profile
	./target/release/profile
	go tool pprof -svg profile.pb

.PHONY: bench
bench: $(CMARK_BIN) ## Run benchmarks
	cargo bench -q
	@ echo ""
	@ cd benches && LD_LIBRARY_PATH=${LD_LIBRARY_PATH}:./cmark-master/build/src ./cmark_benchmark
	@ echo ""
	@ cd benches && go run ./goldmark_benchmark.go

./benches/cmark-master/Makefile:
	cd benches && wget -nc -O cmark.zip https://github.com/commonmark/cmark/archive/master.zip
	cd benches && unzip cmark.zip
	cd benches && rm -f cmark.zip
	cd benches/cmark-master && make

$(CMARK_BIN): ./benches/cmark-master/Makefile benches/cmark_benchmark.c
	gcc -I./benches/cmark-master/build/src -I./benches/cmark-master/src  benches/cmark_benchmark.c -o $(CMARK_BIN) -L./benches/cmark-master/build/src -lcmark; 


.PHONY: clean
clean: ## Clean up generated files
	$(RM) flamegraph.svg
	$(RM) perf.data*
	$(RM) profile*

.PHONY: help
help: ## Show this help
	@sed "s/\$$(APP_NAME)/$(APP_NAME)/g" $(MAKEFILE_LIST) | \
	awk -F':.*##' '/^[^# \t][^:]*:.*##/ { \
	        split($$1, targets, ":"); \
	        gsub(/^[ \t]+|[ \t]+$$/, "", targets[1]); \
	        gsub(/^[ \t]+|[ \t]+$$/, "", $$2); \
	        printf "%-20s %s\n", targets[1] ":", $$2 \
	}'
