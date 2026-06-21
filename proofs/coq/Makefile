# lling-llang Coq/Rocq proofs Makefile
#
# Build with resource limiting (recommended):
#   systemd-run --user --scope -p MemoryMax=126G -p CPUQuota=1800% \
#     -p IOWeight=30 -p TasksMax=200 make -j1
#
# Build without resource limiting:
#   make

COQMAKEFILE = CoqMakefile
COQMAKE = $(MAKE) -f $(COQMAKEFILE)

.PHONY: all clean force proof-check

all: $(COQMAKEFILE)
	$(COQMAKE)

$(COQMAKEFILE): _CoqProject
	coq_makefile -f _CoqProject -o $@

clean:
	if [ -f $(COQMAKEFILE) ]; then $(COQMAKE) clean; fi
	rm -f $(COQMAKEFILE) $(COQMAKEFILE).conf
	rm -f *.glob *.vo *.vok *.vos *.aux
	rm -f foundations/*.glob foundations/*.vo foundations/*.vok foundations/*.vos foundations/*.aux
	rm -f wfst/*.glob wfst/*.vo wfst/*.vok wfst/*.vos wfst/*.aux
	rm -f algorithms/*.glob algorithms/*.vo algorithms/*.vok algorithms/*.vos algorithms/*.aux
	rm -f logict/*.glob logict/*.vo logict/*.vok logict/*.vos logict/*.aux
	rm -f presburger/*.glob presburger/*.vo presburger/*.vok presburger/*.vos presburger/*.aux
	rm -f sft/*.glob sft/*.vo sft/*.vok sft/*.vos sft/*.aux
	rm -f symbolic_algebra/*.glob symbolic_algebra/*.vo symbolic_algebra/*.vok symbolic_algebra/*.vos symbolic_algebra/*.aux

force:
	touch _CoqProject
	$(MAKE) all

# Check for Admitted proofs (CI integration)
proof-check:
	@echo "Checking for unchecked proof escapes..."
	@if grep -REn "^[[:space:]]*(Admitted|Abort)\.|^[[:space:]]*(Axiom|Conjecture|Parameter)[[:space:]]|\badmit\.|True[[:space:]]*\(\*|True\." foundations wfst algorithms logict presburger sft symbolic_algebra 2>/dev/null; then \
		echo "ERROR: Found unchecked or vacuous proof escapes."; \
		exit 1; \
	else \
		echo "No unchecked proof escapes found."; \
	fi
