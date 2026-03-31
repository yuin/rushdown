WASM_EXEC := $(shell go env GOROOT)/lib/wasm/wasm_exec.js
OUT_DIR=../../docs/playground

all: $(OUT_DIR)/rushdown_wasm_bg.wasm $(OUT_DIR)/rushdown_wasm.js $(OUT_DIR)/index.html $(OUT_DIR)/playground.css

.PHONY: run
run:
	python3 -m http.server --directory $(OUT_DIR)

$(OUT_DIR)/index.html: index.html $(OUT_DIR)/rushdown_wasm_bg.wasm $(OUT_DIR)/rushdown_wasm.js
	mkdir -p $(@D) 
	version=$$(cat wasm/Cargo.toml | grep '^rushdown' | awk '{print $$3}' | sed -e 's/"//g'); cat $< | sed -e "s!{{version}}!$${version}!g" > $@


$(OUT_DIR)/playground.css: playground.css
	mkdir -p $(@D) 
	cp $< $@

.PHONY: $(OUT_DIR)/rushdown_wasm_bg.wasm
$(OUT_DIR)/rushdown_wasm_bg.wasm:
	mkdir -p $(@D) 
	cd wasm; make
	cp wasm/pkg/rushdown_wasm_bg.wasm $@

.PHONY: $(OUT_DIR)/rushdown_wasm.js
$(OUT_DIR)/rushdown_wasm.js:
	mkdir -p $(@D) 
	cd wasm; make
	cp wasm/pkg/rushdown_wasm.js $@

.PHONY: clean
clean:
	rm -rf $(OUT_DIR)
