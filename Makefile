.PHONY: verify-proofs diagrams diagrams-force diagrams-check help

help:                ## list available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | \
	  awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'

verify-proofs:       ## run the Coq/Rocq + TLA⁺ proof verification suite
	bash proofs/verify.sh

diagrams:            ## render changed diagram sources under docs/diagrams/ to SVG
	./docs/diagrams/render.sh

diagrams-force:      ## re-render every diagram unconditionally (e.g. after a palette change)
	./docs/diagrams/render.sh --force

diagrams-check:      ## validate every diagram source, write nothing (CI gate)
	./docs/diagrams/render.sh --check
